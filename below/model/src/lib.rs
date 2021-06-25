// Copyright (c) Facebook, Inc. and its affiliates.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[macro_use]
pub mod collector;
pub mod cgroup;
pub mod network;
pub mod process;
pub mod sample;
mod sample_model;
pub mod system;

pub use cgroup::*;
pub use collector::*;
pub use network::*;
pub use process::*;
pub use sample::*;
pub use system::*;

#[cfg(fbcode_build)]
mod facebook;
#[cfg(fbcode_build)]
pub use crate::facebook::*;

/// A wrapper for different field types used in Models. By this way we can query
/// different fields in a single function without using Box.
#[derive(Clone, Debug)]
pub enum Field {
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
    F64(f64),
    Str(String),
    PidState(procfs::PidState),
}

impl From<Field> for i64 {
    fn from(field: Field) -> i64 {
        match field {
            Field::I32(v) => v as i64,
            Field::I64(v) => v as i64,
            _ => panic!("Operation for unsupported types"),
        }
    }
}

impl From<Field> for f64 {
    fn from(field: Field) -> f64 {
        match field {
            Field::U32(v) => v as f64,
            Field::U64(v) => v as f64,
            Field::I32(v) => v as f64,
            Field::I64(v) => v as f64,
            Field::F64(v) => v,
            _ => panic!("Operation for unsupported types"),
        }
    }
}

impl From<Field> for String {
    fn from(field: Field) -> String {
        match field {
            Field::Str(v) => v,
            _ => panic!("Operation for unsupported types"),
        }
    }
}

impl From<u32> for Field {
    fn from(v: u32) -> Self {
        Field::U32(v)
    }
}

impl From<u64> for Field {
    fn from(v: u64) -> Self {
        Field::U64(v)
    }
}

impl From<i32> for Field {
    fn from(v: i32) -> Self {
        Field::I32(v)
    }
}

impl From<i64> for Field {
    fn from(v: i64) -> Self {
        Field::I64(v)
    }
}

impl From<f64> for Field {
    fn from(v: f64) -> Self {
        Field::F64(v)
    }
}

impl From<String> for Field {
    fn from(v: String) -> Self {
        Field::Str(v)
    }
}

impl From<procfs::PidState> for Field {
    fn from(v: procfs::PidState) -> Self {
        Field::PidState(v)
    }
}

impl<T: Into<Field> + Clone> From<&T> for Field {
    fn from(v: &T) -> Self {
        v.clone().into()
    }
}

impl std::ops::Add for Field {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Field::U64(s), Field::U64(o)) => (s + o).into(),
            (Field::I32(s), Field::I32(o)) => (s + o).into(),
            (Field::I64(s), Field::I64(o)) => (s + o).into(),
            (Field::F64(s), Field::F64(o)) => (s + o).into(),
            (Field::Str(s), Field::Str(o)) => (s + &o).into(),
            _ => panic!("Operation for unsupported types"),
        }
    }
}

impl PartialEq for Field {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Field::U64(s), Field::U64(o)) => s == o,
            (Field::I32(s), Field::I32(o)) => s == o,
            (Field::I64(s), Field::I64(o)) => s == o,
            (Field::F64(s), Field::F64(o)) => s == o,
            (Field::Str(s), Field::Str(o)) => s == o,
            (Field::PidState(s), Field::PidState(o)) => s == o,
            _ => false,
        }
    }
}

impl PartialOrd for Field {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Field::U64(s), Field::U64(o)) => s.partial_cmp(o),
            (Field::I32(s), Field::I32(o)) => s.partial_cmp(o),
            (Field::I64(s), Field::I64(o)) => s.partial_cmp(o),
            (Field::F64(s), Field::F64(o)) => s.partial_cmp(o),
            (Field::Str(s), Field::Str(o)) => s.partial_cmp(o),
            (Field::PidState(s), Field::PidState(o)) => s.partial_cmp(o),
            _ => None,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::U32(v) => v.fmt(f),
            Field::U64(v) => v.fmt(f),
            Field::I32(v) => v.fmt(f),
            Field::I64(v) => v.fmt(f),
            Field::F64(v) => v.fmt(f),
            Field::Str(v) => v.fmt(f),
            Field::PidState(v) => v.fmt(f),
        }
    }
}

/// Each Model is composed of Fields and optionally sub-Models. The Queriable
/// trait let us query() a Model for a particular Field within the hierarchy
/// with the given FieldId.
pub trait Queriable {
    type FieldId: FieldId<Queriable = Self>;
    fn query(&self, field_id: &Self::FieldId) -> Option<Field>;
}

/// Marker trait to bind FieldId back to Queriable for type inference.
pub trait FieldId: Sized {
    type Queriable: Queriable<FieldId = Self> + ?Sized;
}

pub fn sort_queriables<T: Queriable>(queriables: &mut [&T], field_id: &T::FieldId, reverse: bool) {
    queriables.sort_by(|lhs, rhs| {
        let order = lhs
            .query(field_id)
            .partial_cmp(&rhs.query(field_id))
            .unwrap_or(std::cmp::Ordering::Equal);
        if reverse { order.reverse() } else { order }
    });
}

/// Models containing sub-Models with its own type, similar to a node in a tree.
/// Such Model has a depth value for illustrating the tree hierarchy.
pub trait Recursive {
    fn get_depth(&self) -> usize;
}

#[derive(Clone, Debug, PartialEq)]
pub struct VecFieldId<Q: Queriable> {
    pub idx: usize,
    pub subquery_id: Q::FieldId,
}

impl<Q: Queriable + Sized> FieldId for VecFieldId<Q> {
    type Queriable = Vec<Q>;
}

/// Placeholder methods in case they are moved to a trait later.
impl<Q: Queriable> VecFieldId<Q> {
    pub fn unit_variant_iter() -> impl std::iter::Iterator<Item = Self> {
        std::iter::empty()
    }
    pub fn all_variant_iter() -> impl std::iter::Iterator<Item = Self> {
        std::iter::empty()
    }
}

impl<Q: Queriable> std::string::ToString for VecFieldId<Q>
where
    <Q as Queriable>::FieldId: std::string::ToString,
{
    fn to_string(&self) -> String {
        format!("{}.{}", self.idx, self.subquery_id.to_string())
    }
}

impl<Q: Queriable> std::str::FromStr for VecFieldId<Q>
where
    <Q as Queriable>::FieldId: std::str::FromStr,
    <<Q as Queriable>::FieldId as std::str::FromStr>::Err: Into<anyhow::Error>,
{
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        if let Some(dot_idx) = s.find('.') {
            Ok(Self {
                idx: s[..dot_idx].parse()?,
                subquery_id: Q::FieldId::from_str(&s[dot_idx + 1..]).map_err(Into::into)?,
            })
        } else {
            Err(anyhow!(
                "Unable to find a variant of the given enum matching string `{}`.",
                s,
            ))
        }
    }
}

impl<T: Queriable> Queriable for Vec<T> {
    type FieldId = VecFieldId<T>;
    fn query(&self, field_id: &Self::FieldId) -> Option<Field> {
        self.get(field_id.idx)
            .and_then(|f| f.query(&field_id.subquery_id))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Model {
    pub time_elapsed: Duration,
    pub timestamp: SystemTime,
    pub system: SystemModel,
    pub cgroup: CgroupModel,
    pub process: ProcessModel,
    pub network: NetworkModel,
}

impl Model {
    /// Construct a `Model` from a Sample and optionally, the last
    /// `CumulativeSample` as well as the `Duration` since it was
    /// collected.
    pub fn new(timestamp: SystemTime, sample: &Sample, last: Option<(&Sample, Duration)>) -> Self {
        Model {
            time_elapsed: last.map(|(_, d)| d).unwrap_or_default(),
            timestamp,
            system: SystemModel::new(&sample.system, last.map(|(s, d)| (&s.system, d))),
            cgroup: CgroupModel::new(
                "<root>".to_string(),
                String::new(),
                0,
                &sample.cgroup,
                last.map(|(s, d)| (&s.cgroup, d)),
            )
            .aggr_top_level_val(),
            process: ProcessModel::new(&sample.processes, last.map(|(s, d)| (&s.processes, d))),
            network: NetworkModel::new(&sample.netstats, last.map(|(s, d)| (&s.netstats, d))),
        }
    }
}

/// Get a sample `Model`. There are no guarantees internal consistency of the
/// model, neither are values in the model supposed to be realistic.
pub fn get_sample_model() -> Model {
    serde_json::from_str(sample_model::SAMPLE_MODEL_JSON)
        .expect("Failed to deserialize sample model JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_sample_model_json() {
        get_sample_model();
    }
}
