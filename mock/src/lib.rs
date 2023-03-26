//! Mock types and functions to generate GethData used for tests

use eth_types::{
    address, bytecode,
    bytecode::{Bytecode, OpcodeWithData},
    evm_types::OpcodeId,
    Address, Bytes, ToWord, Word,
};
use ethers_signers::LocalWallet;
use lazy_static::lazy_static;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
mod account;
mod block;
pub mod test_ctx;
mod transaction;

pub(crate) use account::MockAccount;
pub(crate) use block::MockBlock;
pub use test_ctx::TestContext;
pub use transaction::{AddrOrWallet, MockTransaction, CORRECT_MOCK_TXS};

lazy_static! {
    /// Mock 1 ETH
    pub static ref MOCK_1_ETH: Word = eth(1);
    /// Mock coinbase value
    pub static ref MOCK_COINBASE: Address =
        address!("0x00000000000000000000000000000000c014ba5e");
    /// Mock gasprice value
    pub static ref MOCK_GASPRICE: Word = Word::from(1u8);
    /// Mock BASEFEE value
    pub static ref MOCK_BASEFEE: Word = Word::zero();
     /// Mock GASLIMIT value
    pub static ref MOCK_GASLIMIT: Word = Word::from(0x2386f26fc10000u64);
    /// Mock chain ID value
    pub static ref MOCK_CHAIN_ID: Word = Word::from(1338u64);
    /// Mock DIFFICULTY value
    pub static ref MOCK_DIFFICULTY: Word = Word::from(0x200000u64);
    /// Mock accounts loaded with ETH to use for test cases.
    pub static ref MOCK_ACCOUNTS: Vec<Address> = vec![
        address!("0x000000000000000000000000000000000cafe111"),
        address!("0x000000000000000000000000000000000cafe222"),
        address!("0x000000000000000000000000000000000cafe333"),
        address!("0x000000000000000000000000000000000cafe444"),
        address!("0x000000000000000000000000000000000cafe555"),
    ];
    /// Mock EVM codes to use for test cases.
    pub static ref MOCK_CODES: Vec<Bytes> = vec![
        Bytes::from([0x60, 0x10, 0x00]), // PUSH1(0x10), STOP
        Bytes::from([0x60, 0x01, 0x60, 0x02, 0x01, 0x00]), // PUSH1(1), PUSH1(2), ADD, STOP
        Bytes::from([0x60, 0x01, 0x60, 0x02, 0x02, 0x00]), // PUSH1(1), PUSH1(2), MUL, STOP
        Bytes::from([0x60, 0x02, 0x60, 0x01, 0x03, 0x00]), // PUSH1(2), PUSH1(1), SUB, STOP
        Bytes::from([0x60, 0x09, 0x60, 0x03, 0x04, 0x00]), // PUSH1(9), PUSH1(3), DIV, STOP
        Bytes::from([0x30; 256]), // ADDRESS * 256
    ];
    /// Mock wallets used to generate correctly signed and hashed Transactions.
    pub static ref MOCK_WALLETS: Vec<LocalWallet> = {
        let mut rng = ChaCha20Rng::seed_from_u64(2u64);
        vec![
            LocalWallet::new(&mut rng),
            LocalWallet::new(&mut rng),
            LocalWallet::new(&mut rng),
    ]
    };
}

/// Generate a [`Word`] which corresponds to a certain amount of ETH.
pub fn eth(x: u64) -> Word {
    Word::from(x) * Word::from(10u64.pow(18))
}

/// Express an amount of ETH in GWei.
pub fn gwei(x: u64) -> Word {
    Word::from(x) * Word::from(10u64.pow(9))
}

/// Struct that holds the parameters for generating mock EVM bytecode.
pub struct MockBytecodeParams {
    /// The address to call with the generated bytecode
    pub address: Address,
    /// The data to be passed as arguments to the contract function.
    pub pushdata: Vec<u8>,
    /// The offset in memory where the return data will be stored.
    pub return_data_offset: usize,
    /// The size of the return data.
    pub return_data_size: usize,
    /// The length of the call data.
    pub call_data_length: usize,
    /// The offset in memory where the call data will be stored.
    pub call_data_offset: usize,
    /// The amount of gas to be used for the contract call.
    pub gas: u64,
    /// The instructions to be executed before the STOP opcode.
    pub instructions_before_stop: Bytecode,
}

/// Set default parameters for MockBytecodeParams
impl Default for MockBytecodeParams {
    fn default() -> Self {
        MockBytecodeParams {
            address: address!("0x0000000000000000000000000000000000000000"),
            pushdata: Vec::new(),
            return_data_offset: 0x00usize,
            return_data_size: 0x00usize,
            call_data_length: 0x00usize,
            call_data_offset: 0x00usize,
            gas: 0x1_0000u64,
            instructions_before_stop: Bytecode::default(),
        }
    }
}

/// Generates mock EVM bytecode that performs a simple contract call
pub fn generate_mock_bytecode(params: MockBytecodeParams) -> Bytecode {
    let mut bytecode = bytecode! {
        // populate memory in the context.
        PUSH32(Word::from_big_endian(&params.pushdata))
        PUSH1(0x00) // offset
        MSTORE
        // call address
        PUSH32(params.return_data_offset) // retLength
        PUSH32(params.return_data_size) // retOffset
        PUSH32(params.call_data_length) // argsLength
        PUSH32(params.call_data_offset) // argsOffset
        PUSH1(0x00) // value
        PUSH32(params.address.to_word()) // address
        PUSH32(params.gas) // gas
        CALL
    };
    bytecode.append(&params.instructions_before_stop);
    bytecode.append(&bytecode! {
        STOP
    });
    bytecode
}

// Generates EVM bytecode that loads the balance of a given address
// and performs an operation specified by the opcode.
pub fn generate_mock_balance_bytecode(address: Address, opcode: OpcodeId) -> eth_types::Bytecode {
    let mut bytecode = bytecode! {
        PUSH20(address.to_word())
        BALANCE
    };
    bytecode.append_op(OpcodeWithData::Opcode(opcode));
    bytecode
}

/// Generates mock EVM bytecode with the `RETURN` instruction.
pub fn generate_mock_return_bytecode(
    pushdata: Vec<u8>,
    return_offset: usize,
    return_data_offset: usize,
    return_data_size: usize,
) -> Bytecode {
    bytecode! {
        PUSH32(Word::from_big_endian(&pushdata))
        PUSH32(return_offset)
        MSTORE
        PUSH32(return_data_size)
        PUSH32(return_data_offset)
        RETURN
        STOP
    }
}
