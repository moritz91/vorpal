//! Session-local, typed provenance for references spanning physical files.

use crate::intern::NameId;
use rustc_hash::FxHashMap;
use std::marker::PhantomData;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextId<'i>(NonZeroU32, PhantomData<&'i ()>);

impl ContextId<'_> {
  pub(crate) fn bits(self) -> u32 {
    self.0.get()
  }
  pub(crate) fn from_bits(bits: u32) -> Option<Self> {
    NonZeroU32::new(bits).map(|id| Self(id, PhantomData))
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContextScope<'i> {
  pub root: NameId<'i>,
  pub identity: NameId<'i>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReferenceContext<'i> {
  pub scope: ContextScope<'i>,
  pub owner_external: Option<u128>,
}

#[derive(Default)]
pub(crate) struct Contexts {
  by_key: FxHashMap<(u32, u32, Option<u128>), NonZeroU32>,
  rows: Vec<(u32, u32, Option<u128>)>,
}

impl Contexts {
  pub(crate) fn intern<'i>(&mut self, context: ReferenceContext<'i>) -> ContextId<'i> {
    let key = (
      context.scope.root.to_bits(),
      context.scope.identity.to_bits(),
      context.owner_external,
    );
    let id = *self.by_key.entry(key).or_insert_with(|| {
      let next = u32::try_from(self.rows.len() + 1).expect("reference context overflow");
      self.rows.push(key);
      NonZeroU32::new(next).unwrap()
    });
    ContextId(id, PhantomData)
  }

  pub(crate) fn get(&self, id: ContextId<'_>) -> (u32, u32, Option<u128>) {
    self.rows[id.bits() as usize - 1]
  }
}
