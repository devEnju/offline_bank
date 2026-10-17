//! The ways an earlier release stored the Bank, each named for the last
//! release that stored it so. Its folder reads it with the code that wrote
//! it and assembles the current body from it; nothing is ever written in an
//! earlier way.

use crate::{
    migrate::{Card, Stopped},
    v0_2_1, v0_4_0,
};
use bank_common::bank_files::UnitFile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Up to v0.2.1: one file per container.
    V0_2_1,
    /// v0.3.0 to v0.4.0: the same containers, each unit a file of its own.
    V0_4_0,
}

impl Source {
    pub const ALL: [Self; 2] = [Self::V0_2_1, Self::V0_4_0];

    /// The release, as the result screen names it.
    pub const fn tag(self) -> &'static [u8] {
        match self {
            Self::V0_2_1 => b"v0.2.1",
            Self::V0_4_0 => b"v0.4.0",
        }
    }

    /// Whether a Bank stored this way is on the card.
    pub(crate) fn there<C: Card>(self, card: &mut C) -> Result<bool, C::Error> {
        match self {
            Self::V0_2_1 => v0_2_1::there(card),
            Self::V0_4_0 => v0_4_0::there(card),
        }
    }

    /// Every file of this way, by container. Some have the names of current
    /// files.
    pub(crate) fn files(self) -> [&'static [UnitFile]; 4] {
        match self {
            Self::V0_2_1 => v0_2_1::files(),
            Self::V0_4_0 => v0_4_0::files(),
        }
    }

    /// Reads the Bank into `body` as the current release keeps it. The files
    /// are only read; the Bank must be at rest.
    pub(crate) fn read<C: Card>(
        self,
        card: &mut C,
        body: &mut [u8],
    ) -> Result<(), Stopped<C::Error>> {
        match self {
            Self::V0_2_1 => v0_2_1::read(card, body),
            Self::V0_4_0 => v0_4_0::read(card, body),
        }
    }
}
