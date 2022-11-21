use super::CachedRegion;
use crate::{
    evm_circuit::{
        param::N_BYTES_WORD,
        util::{
            self, constraint_builder::ConstraintBuilder, from_bytes, math_gadget::*, pow_of_two,
            pow_of_two_expr, select, split_u256, split_u256_limb64, sum, Cell,
        },
    },
    util::Expr,
};
use eth_types::{Field, ToLittleEndian, ToScalar, Word};
use halo2_proofs::{
    circuit::Value,
    plonk::{Error, Expression},
};
/// Construction of 256-bit word original and absolute values, which is useful
/// for opcodes operated on signed values.
#[derive(Clone, Debug)]
pub(crate) struct AbsWordGadget<F> {
    x: util::Word<F>,
    x_abs: util::Word<F>,
    sum: util::Word<F>,
    is_neg: LtGadget<F, 1>,
    add_words: AddWordsGadget<F, 2, false>,
}

impl<F: Field> AbsWordGadget<F> {
    pub(crate) fn construct(cb: &mut ConstraintBuilder<F>) -> Self {
        let x = cb.query_word();
        let x_abs = cb.query_word();
        let sum = cb.query_word();
        let x_lo = from_bytes::expr(&x.cells[0..16]);
        let x_hi = from_bytes::expr(&x.cells[16..32]);
        let x_abs_lo = from_bytes::expr(&x_abs.cells[0..16]);
        let x_abs_hi = from_bytes::expr(&x_abs.cells[16..32]);
        let is_neg = LtGadget::construct(cb, 127.expr(), x.cells[31].expr());

        cb.add_constraint(
            "x_abs_lo == x_lo when x >= 0",
            (1.expr() - is_neg.expr()) * (x_abs_lo.expr() - x_lo.expr()),
        );
        cb.add_constraint(
            "x_abs_hi == x_hi when x >= 0",
            (1.expr() - is_neg.expr()) * (x_abs_hi.expr() - x_hi.expr()),
        );

        // When `is_neg`, constrain `sum == 0` and `carry == 1`. Since the final
        // result is `1 << 256`.
        let add_words = AddWordsGadget::construct(cb, [x.clone(), x_abs.clone()], sum.clone());
        cb.add_constraint(
            "sum == 0 when x < 0",
            is_neg.expr() * add_words.sum().expr(),
        );
        cb.add_constraint(
            "carry_hi == 1 when x < 0",
            is_neg.expr() * (1.expr() - add_words.carry().as_ref().unwrap().expr()),
        );

        Self {
            x,
            x_abs,
            sum,
            is_neg,
            add_words,
        }
    }

    pub(crate) fn assign(
        &self,
        region: &mut CachedRegion<'_, '_, F>,
        offset: usize,
        x: Word,
        x_abs: Word,
    ) -> Result<(), Error> {
        self.x.assign(region, offset, Some(x.to_le_bytes()))?;
        self.x_abs
            .assign(region, offset, Some(x_abs.to_le_bytes()))?;
        self.is_neg.assign(
            region,
            offset,
            127.into(),
            u64::from(x.to_le_bytes()[31]).into(),
        )?;
        let sum = x.overflowing_add(x_abs).0;
        self.sum.assign(region, offset, Some(sum.to_le_bytes()))?;
        self.add_words.assign(region, offset, [x, x_abs], sum)
    }

    pub(crate) fn x(&self) -> &util::Word<F> {
        &self.x
    }

    pub(crate) fn x_abs(&self) -> &util::Word<F> {
        &self.x_abs
    }

    pub(crate) fn is_neg(&self) -> &LtGadget<F, 1> {
        &self.is_neg
    }
}

mod tests {
    use super::util::math_gadget::tests::*;
    use super::*;
    use eth_types::Word;
    use halo2_proofs::halo2curves::bn256::Fr;
    use halo2_proofs::plonk::Error;

    #[test]
    fn test_absword() {
        #[derive(Clone)]
        struct AbsWordGadgetContainer<F> {
            absword_gadget: AbsWordGadget<F>,
        }

        impl<F: Field> MathGadgetContainer<F> for AbsWordGadgetContainer<F> {
            const NAME: &'static str = "AbsWordGadget";

            fn configure_gadget_container(cb: &mut ConstraintBuilder<F>) -> Self {
                let absword_gadget = AbsWordGadget::<F>::construct(cb);
                AbsWordGadgetContainer { absword_gadget }
            }

            fn assign_gadget_container(
                &self,
                input_words: &[Word],
                region: &mut CachedRegion<'_, '_, F>,
            ) -> Result<(), Error> {
                let offset = 0;
                let x = input_words[0];
                let x_abs = input_words[1];
                self.absword_gadget.assign(region, offset, x, x_abs)?;

                Ok(())
            }
        }

        test_math_gadget_container::<Fr, AbsWordGadgetContainer<Fr>>(
            vec![Word::from(0), Word::from(0)],
            true,
        );

        test_math_gadget_container::<Fr, AbsWordGadgetContainer<Fr>>(
            vec![Word::from(1), Word::from(1)],
            true,
        );

        test_math_gadget_container::<Fr, AbsWordGadgetContainer<Fr>>(
            vec![Word::from(1), Word::from(2)],
            false,
        );
    }
}
