//! The ways earlier releases stored the Bank, each named for the last
//! release that stored it so, with a folder of its own that fulfils `Way`.
//! A folder reads its way with the code that wrote it and hands the target
//! (`target`) a native body; nothing is ever written in an earlier way.
//!
//! Another way is a folder, a variant here with an arm in each `match`, and
//! its fixtures for the tests that run for every way (docs/building.md).

use crate::{migrate::Card, migrate::Stopped, v0_2_1, v0_4_0};

/// What a folder of an earlier way provides.
pub(crate) trait Way {
    /// The last release that stored the Bank this way, as the result screen
    /// and the marker name it.
    const TAG: &'static [u8];
    /// Whether a Bank stored this way is on the card. Nothing is written.
    fn there<C: Card>(card: &mut C) -> Result<bool, C::Error>;
    /// Reads that Bank into `body` and finishes the body for the target
    /// (`target::into_body`). Its files are only read. A Bank that is not
    /// at rest is refused, as that release would refuse it.
    fn read<C: Card>(card: &mut C, body: &mut [u8]) -> Result<(), Stopped<C::Error>>;
    /// Removes those of its files that are there. Some have the names of
    /// current files.
    fn remove<C: Card>(card: &mut C) -> Result<(), C::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Up to v0.2.1: one file per container.
    V0_2_1,
    /// v0.3.0 to v0.4.0: the same containers, each unit a file of its own.
    V0_4_0,
}

impl Source {
    pub const ALL: [Self; 2] = [Self::V0_2_1, Self::V0_4_0];

    pub const fn tag(self) -> &'static [u8] {
        match self {
            Self::V0_2_1 => <v0_2_1::Way as Way>::TAG,
            Self::V0_4_0 => <v0_4_0::Way as Way>::TAG,
        }
    }

    pub(crate) fn there<C: Card>(self, card: &mut C) -> Result<bool, C::Error> {
        match self {
            Self::V0_2_1 => <v0_2_1::Way as Way>::there(card),
            Self::V0_4_0 => <v0_4_0::Way as Way>::there(card),
        }
    }

    pub(crate) fn read<C: Card>(
        self,
        card: &mut C,
        body: &mut [u8],
    ) -> Result<(), Stopped<C::Error>> {
        match self {
            Self::V0_2_1 => <v0_2_1::Way as Way>::read(card, body),
            Self::V0_4_0 => <v0_4_0::Way as Way>::read(card, body),
        }
    }

    pub(crate) fn remove<C: Card>(self, card: &mut C) -> Result<(), C::Error> {
        match self {
            Self::V0_2_1 => <v0_2_1::Way as Way>::remove(card),
            Self::V0_4_0 => <v0_4_0::Way as Way>::remove(card),
        }
    }

    /// What the tests that run for every way start from.
    #[cfg(test)]
    pub(crate) fn fixtures(self) -> crate::testing::Fixtures {
        match self {
            Self::V0_2_1 => v0_2_1::tests::FIXTURES,
            Self::V0_4_0 => v0_4_0::tests::FIXTURES,
        }
    }
}
