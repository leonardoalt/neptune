use lazy_static::*;

use crate::{
    generate_mds, round_constants, scalar_from_u64, Error, Scalar, ARITY, FULL_ROUNDS,
    PARTIAL_ROUNDS, WIDTH,
};
use ff::{Field, ScalarEngine};
use paired::bls12_381::Bls12;
use std::marker::PhantomData;
lazy_static! {
    pub static ref ARITY_TAG: Scalar = arity_tag::<Bls12>(ARITY);
}

/// The arity tag is the first element of a Poseidon permutation.
/// This extra element is necessary for 128-bit security.
pub fn arity_tag<E: ScalarEngine>(arity: usize) -> E::Fr {
    scalar_from_u64::<E>((1 << arity) - 1)
}

/// The `Poseidon` structure will accept a number of inputs equal to the arity.
///
/// The elements must implement [`ops::Mul`] against a [`Scalar`], because the MDS matrix and the
/// round constants are set, by default, as scalars.
#[derive(Debug, Clone, PartialEq)]
pub struct Poseidon<'a, E: ScalarEngine> {
    constants_offset: usize,
    /// the elements to permute
    pub elements: [E::Fr; WIDTH],
    pos: usize,
    constants: &'a PoseidonConstants<E>,
    _e: PhantomData<E>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PoseidonConstants<E: ScalarEngine> {
    pub mds_matrix: Vec<Vec<E::Fr>>,
    pub round_constants: Vec<E::Fr>,
    pub arity_tag: E::Fr,
    pub width: usize,
}

impl<E: ScalarEngine> PoseidonConstants<E> {
    pub fn new(arity: usize) -> Self {
        let width = arity + 1;
        Self {
            mds_matrix: generate_mds::<E>(width),
            round_constants: round_constants::<E>(width),
            arity_tag: arity_tag::<E>(arity),
            width: width,
        }
    }
}

impl<'a, E: ScalarEngine> Poseidon<'a, E> {
    pub fn new(constants: &'a PoseidonConstants<E>) -> Self {
        let mut elements = [E::Fr::zero(); WIDTH];
        elements[0] = constants.arity_tag;

        Poseidon {
            constants_offset: 0,
            elements: elements,
            pos: 1,
            constants,
            _e: PhantomData::<E>,
        }
    }
    pub fn new_with_preimage(preimage: &[E::Fr], constants: &'a PoseidonConstants<E>) -> Self {
        let mut elements = [constants.arity_tag; WIDTH];
        for i in 1..WIDTH {
            elements[i] = preimage[i - 1];
        }
        Poseidon {
            constants_offset: 0,
            elements: elements,
            pos: WIDTH,
            constants,
            _e: PhantomData::<E>,
        }
    }

    /// Replace the elements with the provided optional items.
    ///
    /// # Panics
    ///
    /// Panics if the provided slice is bigger than the arity.
    pub fn set_preimage(&mut self, preimage: &[E::Fr]) {
        self.reset();
        self.elements[1..].copy_from_slice(&preimage);
    }

    /// Restore the initial state
    pub fn reset(&mut self) {
        self.constants_offset = 0;
        self.elements[1..]
            .iter_mut()
            .for_each(|l| *l = scalar_from_u64::<E>(0u64));
        self.elements[0] = self.constants.arity_tag;
        self.pos = 1;
    }

    /// The returned `usize` represents the element position (within arity) for the input operation
    pub fn input(&mut self, element: E::Fr) -> Result<usize, Error> {
        // Cannot input more elements than the defined arity
        if self.pos >= WIDTH {
            return Err(Error::FullBuffer);
        }

        // Set current element, and increase the pointer
        self.elements[self.pos] = element;
        self.pos += 1;

        Ok(self.pos - 1)
    }

    /// The number of rounds is divided into two equal parts for the full rounds, plus the partial rounds.
    ///
    /// The returned element is the second poseidon element, the first is the arity tag.
    pub fn hash(&mut self) -> E::Fr {
        // This counter is incremented when a round constants is read. Therefore, the round constants never
        // repeat
        for _ in 0..FULL_ROUNDS / 2 {
            self.full_round();
        }

        for _ in 0..PARTIAL_ROUNDS {
            self.partial_round();
        }

        for _ in 0..FULL_ROUNDS / 2 {
            self.full_round();
        }

        self.elements[1]
    }

    /// The full round function will add the round constants and apply the S-Box to all poseidon elements, including the bitflags first element.
    ///
    /// After that, the poseidon elements will be set to the result of the product between the poseidon elements and the constant MDS matrix.
    pub fn full_round(&mut self) {
        // Every element of the hash buffer is incremented by the round constants
        self.add_round_constants();

        // Apply the quintic S-Box to all elements
        self.elements.iter_mut().for_each(|l| quintic_s_box::<E>(l));

        // Multiply the elements by the constant MDS matrix
        self.product_mds();
    }

    /// The partial round is the same as the full round, with the difference that we apply the S-Box only to the first bitflags poseidon leaf.
    pub fn partial_round(&mut self) {
        // Every element of the hash buffer is incremented by the round constants
        self.add_round_constants();

        // Apply the quintic S-Box to the first element
        quintic_s_box::<E>(&mut self.elements[0]);

        // Multiply the elements by the constant MDS matrix
        self.product_mds();
    }

    /// For every leaf, add the round constants with index defined by the constants offset, and increment the
    /// offset
    fn add_round_constants(&mut self) {
        let mut constants_offset = self.constants_offset;

        for i in 0..self.elements.len() {
            self.elements[i].add_assign(&self.constants.round_constants[constants_offset]);
            constants_offset += 1;
        }
        self.constants_offset = constants_offset;
    }

    /// Set the provided elements with the result of the product between the elements and the constant
    /// MDS matrix
    fn product_mds(&mut self) {
        let mut result: [E::Fr; WIDTH] = [scalar_from_u64::<E>(0u64); WIDTH];

        for j in 0..WIDTH {
            for k in 0..WIDTH {
                let mut tmp = self.constants.mds_matrix[j][k];
                tmp.mul_assign(&self.elements[k]);
                result[j].add_assign(&tmp);
            }
        }

        self.elements.copy_from_slice(&result);
    }
}

/// Apply the quintic S-Box (s^5) to a given item
fn quintic_s_box<E: ScalarEngine>(l: &mut E::Fr) {
    let c = *l;
    let mut tmp = l.clone();
    tmp.mul_assign(&c);
    tmp.mul_assign(&tmp.clone());
    l.mul_assign(&tmp);
}

/// Poseidon convenience hash function.
/// NOTE: this is expensive, since it computes all constants when initializing hasher struct.
pub fn poseidon<E: ScalarEngine>(preimage: &[E::Fr]) -> E::Fr {
    assert_eq!(
        ARITY,
        preimage.len(),
        "Preimage should contain {} elements but contained {}.",
        ARITY,
        preimage.len()
    );
    let constants = PoseidonConstants::new(ARITY);
    Poseidon::<E>::new_with_preimage(preimage, &constants).hash()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    use ff::Field;

    #[test]
    fn reset() {
        let preimage: [Scalar; ARITY] = [Scalar::one(); ARITY];
        let constants = PoseidonConstants::new(ARITY);
        let mut h = Poseidon::<Bls12>::new_with_preimage(&preimage, &constants);
        h.hash();
        h.reset();

        let default = Poseidon::<Bls12>::new(&constants);
        assert_eq!(default.pos, h.pos);
        assert_eq!(default.elements, h.elements);
        assert_eq!(default.constants_offset, h.constants_offset);
    }

    #[test]
    fn hash_det() {
        let mut preimage: [Scalar; ARITY] = [Scalar::zero(); ARITY];
        let constants = PoseidonConstants::new(ARITY);
        preimage[0] = Scalar::one();

        let mut h = Poseidon::<Bls12>::new_with_preimage(&preimage, &constants);

        let mut h2 = h.clone();
        let result: <Bls12 as ScalarEngine>::Fr = h.hash();

        assert_eq!(result, h2.hash());
    }

    #[test]
    /// Simple test vectors to ensure results don't change unintentionally in development.
    fn hash_values() {
        let constants = PoseidonConstants::new(ARITY);
        let mut p = Poseidon::<Bls12>::new(&constants);
        let mut preimage = [Scalar::zero(); ARITY];
        for n in 0..ARITY {
            let scalar = scalar_from_u64::<Bls12>(n as u64);
            p.input(scalar).unwrap();
            preimage[n] = scalar;
        }
        let digest = p.hash();
        let expected = match ARITY {
            2 => scalar_from_u64s([
                0x5839abf48eafbcc5,
                0x651ef33cc1fb7943,
                0x8c505814a167b971,
                0x38de26599ba2def0,
            ]),
            4 => scalar_from_u64s([
                0xf491e8e3b2136ea0,
                0x04f40ac4e1cdd09b,
                0xfaf9cfadd283daad,
                0x65a4e5fc9b670f89,
            ]),
            8 => scalar_from_u64s([
                0x61743f58c2ee916a,
                0x07608ceb5fc5a8d5,
                0xc0c06b2302d5392e,
                0x34841de8e928834b,
            ]),
            _ => {
                dbg!(digest);
                panic!("Arity lacks test vector: {}", ARITY)
            }
        };
        dbg!(ARITY);
        assert_eq!(expected, digest);

        assert_eq!(
            digest,
            poseidon::<Bls12>(&preimage),
            "Poseidon wrapper disagrees with element-at-a-time invocation."
        );
    }
}