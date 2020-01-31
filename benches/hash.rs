use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ff::PrimeField;
use neptune::poseidon::PoseidonConstants;
use neptune::*;
use paired::bls12_381::Bls12;
use rand::rngs::OsRng;
use rand::seq::SliceRandom;
use sha2::{Digest, Sha256, Sha512};

fn bench_hash(c: &mut Criterion) {
    let scalars: Vec<Scalar> = std::iter::repeat(())
        .take(1000)
        .enumerate()
        .map(|(i, _)| scalar_from_u64::<Bls12>(i as u64))
        .collect();

    let mut group = c.benchmark_group(format!("hash-{}", ARITY * 32));

    group.bench_with_input(
        BenchmarkId::new("Sha2 256", "Generated scalars"),
        &scalars,
        |b, s| {
            b.iter(|| {
                let mut h = Sha256::new();

                std::iter::repeat(())
                    .take(ARITY)
                    .map(|_| s.choose(&mut OsRng).unwrap())
                    .for_each(|scalar| {
                        for val in scalar.into_repr().as_ref() {
                            h.input(&val.to_le_bytes());
                        }
                    });

                h.result();
            })
        },
    );

    group.bench_with_input(
        BenchmarkId::new("Sha2 512", "Generated scalars"),
        &scalars,
        |b, s| {
            b.iter(|| {
                let mut h = Sha512::new();

                std::iter::repeat(())
                    .take(ARITY)
                    .map(|_| s.choose(&mut OsRng).unwrap())
                    .for_each(|scalar| {
                        for val in scalar.into_repr().as_ref() {
                            h.input(&val.to_le_bytes());
                        }
                    });

                h.result();
            })
        },
    );

    group.bench_with_input(
        BenchmarkId::new("Poseidon hash", "Generated scalars"),
        &scalars,
        |b, s| {
            let constants = PoseidonConstants::new(ARITY);
            let mut h = Poseidon::<Bls12>::new(&constants);
            b.iter(|| {
                h.reset();
                std::iter::repeat(())
                    .take(ARITY)
                    .map(|_| s.choose(&mut OsRng).unwrap())
                    .for_each(|scalar| {
                        h.input(*scalar).unwrap();
                    });

                h.hash();
            })
        },
    );

    group.finish();
}

criterion_group! {
    name = hash;

    config = Criterion::default();

    targets = bench_hash
}
criterion_main!(hash);