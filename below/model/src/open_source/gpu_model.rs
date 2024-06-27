// Copyright (c) Facebook, Inc. and its affiliates.
//
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

use serde::Deserialize;
use serde::Serialize;

use super::*;

#[derive(Default, Clone, Serialize, Deserialize)]
struct Never {}

impl Display for Never {
    fn fmt(&self, _: &mut fmt::Formatter) -> fmt::Result {
        match *self {}
    }
}

#[derive(Default, Clone, Serialize, Deserialize, below_derive::Queriable)]
pub struct GpuModel {
    never: Never,
}

impl GpuModel {
    pub fn new(_sample: &gpu_stats::GpuMap, _last: Option<(&gpu_stats::GpuMap, Duration)>) -> Self {
        // Open source GPU Model not implemented yet
        Self { never: Never {} }
    }
}
