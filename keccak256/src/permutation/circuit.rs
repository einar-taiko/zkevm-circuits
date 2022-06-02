use crate::{
    common::{NEXT_INPUTS_LANES, PERMUTATION},
    permutation::{
        add::AddConfig, base_conversion::BaseConversionConfig, flag::FlagConfig, iota::IotaConfig,
        mixing::MixingConfig, pi::pi_gate_permutation, rho::RhoConfig,
        tables::FromBase9TableConfig, theta::assign_theta, xi::assign_xi,
    },
};
use eth_types::Field;
use halo2_proofs::{
    circuit::{AssignedCell, Layouter},
    plonk::{Advice, Column, ConstraintSystem, Error},
};
use itertools::Itertools;
use std::convert::TryInto;

#[derive(Clone, Debug)]
pub struct KeccakFConfig<F: Field> {
    add: AddConfig<F>,
    rho_config: RhoConfig<F>,
    iota_config: IotaConfig<F>,
    from_b9_table: FromBase9TableConfig<F>,
    base_conversion_config: BaseConversionConfig<F>,
    mixing_config: MixingConfig<F>,
    pub state: [Column<Advice>; 25],
}

impl<F: Field> KeccakFConfig<F> {
    // We assume state is received in base-9.
    pub fn configure(meta: &mut ConstraintSystem<F>) -> Self {
        let state: [Column<Advice>; 25] = (0..25)
            .map(|_| {
                let column = meta.advice_column();
                meta.enable_equality(column);
                column
            })
            .collect_vec()
            .try_into()
            .unwrap();

        let fixed = meta.fixed_column();

        let add = AddConfig::configure(meta, state[0..3].try_into().unwrap(), fixed);
        let flag = FlagConfig::configure(meta, state[0]);

        // rho
        let rho_config = RhoConfig::configure(meta, state, fixed, add.clone());
        let iota_config = IotaConfig::configure(add.clone());

        // Base conversion config.
        let from_b9_table = FromBase9TableConfig::configure(meta);
        let base_info = from_b9_table.get_base_info(false);
        let base_conversion_config =
            BaseConversionConfig::configure(meta, base_info, state[0..2].try_into().unwrap(), &add);

        // Mixing will make sure that the flag is binary constrained and that
        // the out state matches the expected result.
        let mixing_config =
            MixingConfig::configure(meta, &from_b9_table, iota_config.clone(), &add, state, flag);

        Self {
            add,
            rho_config,
            iota_config,
            from_b9_table,
            base_conversion_config,
            mixing_config,
            state,
        }
    }

    pub fn load(&self, layouter: &mut impl Layouter<F>) -> Result<(), Error> {
        self.rho_config.load(layouter)?;
        self.from_b9_table.load(layouter)
    }

    pub fn assign_all(
        &self,
        layouter: &mut impl Layouter<F>,
        in_state: [AssignedCell<F, F>; 25],
        flag: Option<bool>,
        next_mixing: [Option<F>; NEXT_INPUTS_LANES],
    ) -> Result<[AssignedCell<F, F>; 25], Error> {
        let mut state = in_state;

        // First 23 rounds
        for round_idx in 0..PERMUTATION {
            // State in base-13
            // theta
            state = assign_theta(&self.add, layouter, &state)?;

            // rho
            state = self.rho_config.assign_rotation_checks(layouter, &state)?;
            // Outputs in base-9 which is what Pi requires

            // Apply Pi permutation
            state = pi_gate_permutation(&state);

            // xi
            state = assign_xi(&self.add, layouter, &state)?;

            // Last round before Mixing does not run IotaB9 nor BaseConversion
            if round_idx == PERMUTATION - 1 {
                break;
            }

            // iota_b9
            state[0] = self
                .iota_config
                .assign_round_b9(layouter, state[0].clone(), round_idx)?;

            // The resulting state is in Base-9 now. We now convert it to
            // base_13 which is what Theta requires again at the
            // start of the loop.
            state = self.base_conversion_config.assign_state(layouter, &state)?;
        }

        let mix_res = self
            .mixing_config
            .assign_state(layouter, &state, flag, next_mixing)?;
        Ok(mix_res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arith_helpers::*;
    use crate::common::{State, NEXT_INPUTS_LANES};
    use crate::gate_helpers::biguint_to_f;
    use crate::keccak_arith::*;
    use halo2_proofs::circuit::Layouter;
    use halo2_proofs::pairing::bn256::Fr as Fp;
    use halo2_proofs::plonk::{ConstraintSystem, Error};
    use halo2_proofs::{circuit::SimpleFloorPlanner, dev::MockProver, plonk::Circuit};
    use pretty_assertions::assert_eq;
    use std::convert::TryInto;

    // TODO: Remove ignore once this can run in the CI without hanging.
    #[ignore]
    #[test]
    fn test_keccak_round() {
        #[derive(Default)]
        struct MyCircuit<F> {
            in_state: [F; 25],
            out_state: [F; 25],
            next_mixing: [Option<F>; NEXT_INPUTS_LANES],
            // flag
            is_mixing: bool,
        }

        impl<F: Field> Circuit<F> for MyCircuit<F> {
            type Config = KeccakFConfig<F>;
            type FloorPlanner = SimpleFloorPlanner;

            fn without_witnesses(&self) -> Self {
                Self::default()
            }

            fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
                Self::Config::configure(meta)
            }

            fn synthesize(
                &self,
                config: Self::Config,
                mut layouter: impl Layouter<F>,
            ) -> Result<(), Error> {
                // Load the table
                config.load(&mut layouter)?;
                let offset: usize = 0;

                let in_state = layouter.assign_region(
                    || "Keccak round Wittnes & flag assignation",
                    |mut region| {
                        // Witness `state`
                        let in_state: [AssignedCell<F, F>; 25] = {
                            let mut state: Vec<AssignedCell<F, F>> = Vec::with_capacity(25);
                            for (idx, val) in self.in_state.iter().enumerate() {
                                let cell = region.assign_advice(
                                    || "witness input state",
                                    config.state[idx],
                                    offset,
                                    || Ok(*val),
                                )?;
                                state.push(cell)
                            }
                            state.try_into().unwrap()
                        };

                        Ok(in_state)
                    },
                )?;

                let out_state = config.assign_all(
                    &mut layouter,
                    in_state,
                    Some(self.is_mixing),
                    self.next_mixing,
                )?;
                layouter.assign_region(
                    || "State check",
                    |mut region| {
                        for (lane, value) in out_state.iter().zip(self.out_state.iter()) {
                            region.constrain_constant(lane.cell(), value)?;
                        }
                        Ok(())
                    },
                )?;
                Ok(())
            }
        }

        let in_state: State = [
            [1, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ];

        let next_input: State = [
            [2, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0],
        ];

        let mut in_state_biguint = StateBigInt::default();

        // Generate in_state as `[Fp;25]`
        let mut in_state_fp: [Fp; 25] = [Fp::zero(); 25];
        for (x, y) in (0..5).cartesian_product(0..5) {
            in_state_fp[5 * x + y] = biguint_to_f(&convert_b2_to_b13(in_state[x][y]));
            in_state_biguint[(x, y)] = convert_b2_to_b13(in_state[x][y]);
        }

        // Compute out_state_mix
        let mut out_state_mix = in_state_biguint.clone();
        KeccakFArith::permute_and_absorb(&mut out_state_mix, Some(&next_input));

        // Compute out_state_non_mix
        let mut out_state_non_mix = in_state_biguint.clone();
        KeccakFArith::permute_and_absorb(&mut out_state_non_mix, None);

        // Generate out_state as `[Fp;25]`
        let out_state_mix: [Fp; 25] = state_bigint_to_field(out_state_mix);
        let out_state_non_mix: [Fp; 25] = state_bigint_to_field(out_state_non_mix);

        // Generate next_input (tho one that is not None) in the form `[F;17]`
        // Generate next_input as `[Fp;NEXT_INPUTS_LANES]`
        let next_input_fp: [Option<Fp>; NEXT_INPUTS_LANES] =
            state_bigint_to_field::<_, NEXT_INPUTS_LANES>(StateBigInt::from(next_input))
                .iter()
                .map(|&x| Some(x))
                .collect_vec()
                .try_into()
                .unwrap();

        // When we pass no `mixing_inputs`, we perform the full keccak round
        // ending with Mixing executing IotaB9
        {
            // With the correct input and output witnesses, the proof should
            // pass.
            let circuit = MyCircuit::<Fp> {
                in_state: in_state_fp,
                out_state: out_state_non_mix,
                next_mixing: [None; NEXT_INPUTS_LANES],
                is_mixing: false,
            };

            let prover = MockProver::<Fp>::run(17, &circuit, vec![]).unwrap();

            assert_eq!(prover.verify(), Ok(()));

            // With wrong input and/or output witnesses, the proof should fail
            // to be verified.
            let circuit = MyCircuit::<Fp> {
                in_state: out_state_non_mix,
                out_state: out_state_non_mix,
                next_mixing: [None; NEXT_INPUTS_LANES],
                is_mixing: true,
            };
            let k = 17;
            let prover = MockProver::<Fp>::run(k, &circuit, vec![]).unwrap();

            #[cfg(feature = "dev-graph")]
            {
                use plotters::prelude::*;
                let root = BitMapBackend::new("keccak-f.png", (1024, 16384)).into_drawing_area();
                root.fill(&WHITE).unwrap();
                let root = root.titled("Keccak-F", ("sans-serif", 60)).unwrap();
                halo2_proofs::dev::CircuitLayout::default()
                    .show_labels(false)
                    .render(k, &circuit, &root)
                    .unwrap();
            }

            assert!(prover.verify().is_err());
        }

        // When we pass `mixing_inputs`, we perform the full keccak round ending
        // with Mixing executing Absorb + base_conversion + IotaB13
        {
            let circuit = MyCircuit::<Fp> {
                in_state: in_state_fp,
                out_state: out_state_mix,
                next_mixing: next_input_fp,
                is_mixing: true,
            };

            let prover = MockProver::<Fp>::run(17, &circuit, vec![]).unwrap();

            assert_eq!(prover.verify(), Ok(()));

            // With wrong input and/or output witnesses, the proof should fail
            // to be verified.
            let circuit = MyCircuit::<Fp> {
                in_state: out_state_non_mix,
                out_state: out_state_non_mix,
                next_mixing: next_input_fp,
                is_mixing: true,
            };

            let prover = MockProver::<Fp>::run(17, &circuit, vec![]).unwrap();

            assert!(prover.verify().is_err());
        }
    }
}
