use std::sync::Arc;

use beserial::Deserialize;
use nimiq_block_albatross::{
    Block, MacroBlock, MacroBody, PbftCommitMessage, PbftPrepareMessage, PbftProofBuilder,
    PbftProposal, SignedPbftCommitMessage, SignedPbftPrepareMessage,
};
use nimiq_block_production_albatross::BlockProducer;
use nimiq_blockchain_albatross::{Blockchain, Direction, PushResult};
use nimiq_bls::{KeyPair, SecretKey};
use nimiq_database::volatile::VolatileEnvironment;
use nimiq_genesis::NetworkId;
use nimiq_hash::{Blake2bHash, Hash};
use nimiq_primitives::policy;



/// Secret key of validator. Tests run with `genesis/src/genesis/unit-albatross.toml`
const SECRET_KEY: &'static str = "196ffdb1a8acc7cbd76a251aeac0600a1d68b3aba1eba823b5e4dc5dbdcdc730afa752c05ab4f6ef8518384ad514f403c5a088a22b17bf1bc14f8ff8decc2a512c0a200f68d7bdf5a319b30356fe8d1d75ef510aed7a8660968c216c328a0000";

// Fill epoch with micro blocks
fn fill_micro_blocks(producer: &BlockProducer, blockchain: &Arc<Blockchain>) {
    let init_height = blockchain.block_number();
    let macro_block_number = policy::macro_block_after(init_height + 1);
    for i in (init_height + 1)..macro_block_number {
        let last_micro_block = producer.next_micro_block(
            blockchain.time.now() + i as u64 * 1000,
            0,
            None,
            vec![],
            vec![0x42],
        );
        assert_eq!(
            blockchain.push(Block::Micro(last_micro_block)),
            Ok(PushResult::Extended)
        );
    }
    assert_eq!(blockchain.block_number(), macro_block_number - 1);
}

fn produce_macro_blocks(num_macro: usize, producer: &BlockProducer, blockchain: &Arc<Blockchain>) {
    for _ in 0..num_macro {
        fill_micro_blocks(producer, blockchain);

        let next_block_height = blockchain.block_number() + 1;
        let (proposal, extrinsics) = producer.next_macro_block_proposal(
            blockchain.time.now() + next_block_height as u64 * 1000,
            0u32,
            None,
            vec![],
        );

        let block = sign_macro_block(proposal, extrinsics);
        assert_eq!(
            blockchain.push(Block::Macro(block)),
            Ok(PushResult::Extended)
        );
    }
}

fn sign_macro_block(proposal: PbftProposal, extrinsics: MacroBody) -> MacroBlock {
    let keypair =
        KeyPair::from(SecretKey::deserialize_from_vec(&hex::decode(SECRET_KEY).unwrap()).unwrap());

    let block_hash = proposal.header.hash::<Blake2bHash>();

    // create signed prepare and commit
    let prepare = SignedPbftPrepareMessage::from_message(
        PbftPrepareMessage {
            block_hash: block_hash.clone(),
        },
        &keypair.secret_key,
        0,
    );
    let commit = SignedPbftCommitMessage::from_message(
        PbftCommitMessage {
            block_hash: block_hash.clone(),
        },
        &keypair.secret_key,
        0,
    );

    // create proof
    let mut pbft_proof = PbftProofBuilder::new();
    pbft_proof.add_prepare_signature(&keypair.public_key, policy::SLOTS, &prepare);
    pbft_proof.add_commit_signature(&keypair.public_key, policy::SLOTS, &commit);

    MacroBlock {
        header: proposal.header,
        justification: Some(pbft_proof.build()),
        body: Some(extrinsics),
    }
}

// FIXME: Enable this test when history root refactor is ready.
// #[test]
fn it_can_sync_macro_blocks() {
    let env = VolatileEnvironment::new(10).unwrap();
    let blockchain = Arc::new(Blockchain::new(env.clone(), NetworkId::UnitAlbatross).unwrap());
    let genesis_hash = blockchain.head_hash();

    let keypair =
        KeyPair::from(SecretKey::deserialize_from_vec(&hex::decode(SECRET_KEY).unwrap()).unwrap());
    let producer = BlockProducer::new_without_mempool(Arc::clone(&blockchain), keypair);

    produce_macro_blocks(2, &producer, &blockchain);

    let macro_blocks = blockchain
        .chain_store
        .get_macro_blocks(&genesis_hash, 10, true, Direction::Forward, false, None)
        .unwrap();
    assert_eq!(macro_blocks.len(), 2);

    // Create a second blockchain to push these blocks.
    let env2 = VolatileEnvironment::new(10).unwrap();
    let blockchain2 = Arc::new(Blockchain::new(env2.clone(), NetworkId::UnitAlbatross).unwrap());

    for block in macro_blocks {
        assert_eq!(
            blockchain2.push_isolated_macro_block(block, &[]),
            Ok(PushResult::Extended)
        );
    }
}

// TODO Test transactions
