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

use super::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize, below_derive::Queriable)]
pub struct CgroupModel {
    pub name: String,
    pub full_path: String,
    pub inode_number: Option<u64>,
    #[queriable(ignore)]
    pub depth: u32,
    #[queriable(subquery)]
    pub cpu: Option<CgroupCpuModel>,
    #[queriable(subquery)]
    #[queriable(preferred_name = mem)]
    pub memory: Option<CgroupMemoryModel>,
    #[queriable(ignore)]
    pub io: Option<BTreeMap<String, CgroupIoModel>>,
    #[queriable(subquery)]
    #[queriable(preferred_name = io)]
    pub io_total: Option<CgroupIoModel>,
    #[queriable(subquery)]
    pub pressure: Option<CgroupPressureModel>,
    #[queriable(ignore)]
    pub children: BTreeSet<CgroupModel>,
    #[queriable(ignore)]
    pub count: u32,
    // Indicate if such cgroup is created
    #[queriable(ignore)]
    pub recreate_flag: bool,
}

// We implement equality and ordering based on the cgroup name only so
// CgroupModel can be stored in a BTreeSet
impl Ord for CgroupModel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialOrd for CgroupModel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for CgroupModel {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for CgroupModel {}

impl CgroupModel {
    pub fn new(
        name: String,
        full_path: String,
        depth: u32,
        sample: &CgroupSample,
        last: Option<(&CgroupSample, Duration)>,
    ) -> CgroupModel {
        let last_if_inode_matches =
            last.and_then(|(s, d)| match (s.inode_number, sample.inode_number) {
                (Some(prev_inode), Some(current_inode)) if prev_inode == current_inode => {
                    Some((s, d))
                }
                (None, None) => Some((s, d)),
                _ => None,
            });
        let (cpu, io, io_total, recreate_flag) = if let Some((last, delta)) = last_if_inode_matches
        {
            // We have cumulative data, create cpu, io models
            let cpu = match (last.cpu_stat.as_ref(), sample.cpu_stat.as_ref()) {
                (Some(begin), Some(end)) => Some(CgroupCpuModel::new(begin, end, delta)),
                _ => None,
            };
            let io = match (last.io_stat.as_ref(), sample.io_stat.as_ref()) {
                (Some(begin), Some(end)) => Some(
                    end.iter()
                        .filter_map(|(device_name, end_io_stat)| {
                            begin.get(device_name).map(|begin_io_stat| {
                                (
                                    device_name.clone(),
                                    CgroupIoModel::new(&begin_io_stat, &end_io_stat, delta),
                                )
                            })
                        })
                        .collect::<BTreeMap<String, CgroupIoModel>>(),
                ),
                _ => None,
            };
            let io_total = io.as_ref().map(|io_map| {
                io_map
                    .iter()
                    .fold(CgroupIoModel::empty(), |acc, (_, model)| acc + model)
            });

            (cpu, io, io_total, false)
        } else {
            // No cumulative data or inode number is different
            (None, None, None, last.is_some())
        };

        let memory = Some(CgroupMemoryModel::new(sample, last));

        let pressure = sample
            .pressure
            .as_ref()
            .map(|p| CgroupPressureModel::new(p));

        // recursively calculate view of children
        // `children` is optional, but we treat it the same as an empty map
        let empty = BTreeMap::new();
        let children = sample
            .children
            .as_ref()
            .unwrap_or(&empty)
            .iter()
            .map(|(child_name, child_sample)| {
                CgroupModel::new(
                    child_name.clone(),
                    format!("{}/{}", full_path, child_name),
                    depth + 1,
                    &child_sample,
                    last.and_then(|(last, delta)| {
                        last.children
                            .as_ref()
                            .unwrap_or(&empty)
                            .get(child_name)
                            .map(|child_last| (child_last, delta))
                    }),
                )
            })
            .collect::<BTreeSet<CgroupModel>>();
        let nr_descendants: u32 = children.iter().fold(0, |acc, c| acc + c.count);
        CgroupModel {
            name,
            full_path,
            inode_number: sample.inode_number.map(|ino| ino as u64),
            cpu,
            memory,
            io,
            io_total,
            pressure,
            children,
            count: nr_descendants + 1,
            depth,
            recreate_flag,
        }
    }

    pub fn aggr_top_level_val(mut self) -> Self {
        self.memory = self.children.iter().fold(Default::default(), |acc, model| {
            opt_add(acc, model.memory.clone())
        });
        self
    }
}

impl Recursive for CgroupModel {
    fn get_depth(&self) -> usize {
        self.depth as usize
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    below_derive::Queriable
)]
pub struct CgroupCpuModel {
    pub usage_pct: Option<f64>,
    pub user_pct: Option<f64>,
    pub system_pct: Option<f64>,
    pub nr_periods_per_sec: Option<f64>,
    pub nr_throttled_per_sec: Option<f64>,
    pub throttled_pct: Option<f64>,
}

impl CgroupCpuModel {
    pub fn new(
        begin: &cgroupfs::CpuStat,
        end: &cgroupfs::CpuStat,
        delta: Duration,
    ) -> CgroupCpuModel {
        CgroupCpuModel {
            usage_pct: usec_pct!(begin.usage_usec, end.usage_usec, delta),
            user_pct: usec_pct!(begin.user_usec, end.user_usec, delta),
            system_pct: usec_pct!(begin.system_usec, end.system_usec, delta),
            nr_periods_per_sec: count_per_sec!(begin.nr_periods, end.nr_periods, delta),
            nr_throttled_per_sec: count_per_sec!(begin.nr_throttled, end.nr_throttled, delta),
            throttled_pct: usec_pct!(begin.throttled_usec, end.throttled_usec, delta),
        }
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    below_derive::Queriable
)]
pub struct CgroupIoModel {
    pub rbytes_per_sec: Option<f64>,
    pub wbytes_per_sec: Option<f64>,
    pub rios_per_sec: Option<f64>,
    pub wios_per_sec: Option<f64>,
    pub dbytes_per_sec: Option<f64>,
    pub dios_per_sec: Option<f64>,
    pub rwbytes_per_sec: Option<f64>,
}

impl CgroupIoModel {
    pub fn new(begin: &cgroupfs::IoStat, end: &cgroupfs::IoStat, delta: Duration) -> CgroupIoModel {
        let rbytes_per_sec = count_per_sec!(begin.rbytes, end.rbytes, delta);
        let wbytes_per_sec = count_per_sec!(begin.wbytes, end.wbytes, delta);
        let rwbytes_per_sec = opt_add(rbytes_per_sec.clone(), wbytes_per_sec.clone());
        CgroupIoModel {
            rbytes_per_sec,
            wbytes_per_sec,
            rios_per_sec: count_per_sec!(begin.rios, end.rios, delta),
            wios_per_sec: count_per_sec!(begin.wios, end.wios, delta),
            dbytes_per_sec: count_per_sec!(begin.dbytes, end.dbytes, delta),
            dios_per_sec: count_per_sec!(begin.dios, end.dios, delta),
            rwbytes_per_sec,
        }
    }

    pub fn empty() -> CgroupIoModel {
        // If io.stat file is empty, it means cgroup has no I/O at all. In that
        // case we default to zero instead of None.
        CgroupIoModel {
            rbytes_per_sec: Some(0.0),
            wbytes_per_sec: Some(0.0),
            rios_per_sec: Some(0.0),
            wios_per_sec: Some(0.0),
            dbytes_per_sec: Some(0.0),
            dios_per_sec: Some(0.0),
            rwbytes_per_sec: Some(0.0),
        }
    }
}

impl std::ops::Add<&CgroupIoModel> for CgroupIoModel {
    type Output = Self;

    fn add(self, other: &Self) -> Self {
        Self {
            rbytes_per_sec: opt_add(self.rbytes_per_sec, other.rbytes_per_sec),
            wbytes_per_sec: opt_add(self.wbytes_per_sec, other.wbytes_per_sec),
            rios_per_sec: opt_add(self.rios_per_sec, other.rios_per_sec),
            wios_per_sec: opt_add(self.wios_per_sec, other.wios_per_sec),
            dbytes_per_sec: opt_add(self.dbytes_per_sec, other.dbytes_per_sec),
            dios_per_sec: opt_add(self.dios_per_sec, other.dios_per_sec),
            rwbytes_per_sec: opt_add(self.rwbytes_per_sec, other.rwbytes_per_sec),
        }
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    below_derive::Queriable
)]
pub struct CgroupMemoryModel {
    pub total: Option<u64>,
    pub swap: Option<u64>,
    pub anon: Option<u64>,
    pub file: Option<u64>,
    pub kernel_stack: Option<u64>,
    pub slab: Option<u64>,
    pub sock: Option<u64>,
    pub shmem: Option<u64>,
    pub file_mapped: Option<u64>,
    pub file_dirty: Option<u64>,
    pub file_writeback: Option<u64>,
    pub anon_thp: Option<u64>,
    pub inactive_anon: Option<u64>,
    pub active_anon: Option<u64>,
    pub inactive_file: Option<u64>,
    pub active_file: Option<u64>,
    pub unevictable: Option<u64>,
    pub slab_reclaimable: Option<u64>,
    pub slab_unreclaimable: Option<u64>,
    pub pgfault: Option<u64>,
    pub pgmajfault: Option<u64>,
    pub workingset_refault: Option<u64>,
    pub workingset_activate: Option<u64>,
    pub workingset_nodereclaim: Option<u64>,
    pub pgrefill: Option<u64>,
    pub pgscan: Option<u64>,
    pub pgsteal: Option<u64>,
    pub pgactivate: Option<u64>,
    pub pgdeactivate: Option<u64>,
    pub pglazyfree: Option<u64>,
    pub pglazyfreed: Option<u64>,
    pub thp_fault_alloc: Option<u64>,
    pub thp_collapse_alloc: Option<u64>,
    pub memory_high: Option<i64>,
    pub events_low: Option<u64>,
    pub events_high: Option<u64>,
    pub events_max: Option<u64>,
    pub events_oom: Option<u64>,
    pub events_oom_kill: Option<u64>,
}

impl std::ops::Add for CgroupMemoryModel {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        Self {
            total: opt_add(self.total, other.total),
            swap: opt_add(self.swap, other.swap),
            anon: opt_add(self.anon, other.anon),
            file: opt_add(self.file, other.file),
            kernel_stack: opt_add(self.kernel_stack, other.kernel_stack),
            slab: opt_add(self.slab, other.slab),
            sock: opt_add(self.sock, other.sock),
            shmem: opt_add(self.shmem, other.shmem),
            file_mapped: opt_add(self.file_mapped, other.file_mapped),
            file_dirty: opt_add(self.file_dirty, other.file_dirty),
            file_writeback: opt_add(self.file_writeback, other.file_writeback),
            anon_thp: opt_add(self.anon_thp, other.anon_thp),
            inactive_anon: opt_add(self.inactive_anon, other.inactive_anon),
            active_anon: opt_add(self.active_anon, other.active_anon),
            inactive_file: opt_add(self.inactive_file, other.inactive_file),
            active_file: opt_add(self.active_file, other.active_file),
            unevictable: opt_add(self.unevictable, other.unevictable),
            slab_reclaimable: opt_add(self.slab_reclaimable, other.slab_reclaimable),
            slab_unreclaimable: opt_add(self.slab_unreclaimable, other.slab_unreclaimable),
            pgfault: opt_add(self.pgfault, other.pgfault),
            pgmajfault: opt_add(self.pgmajfault, other.pgmajfault),
            workingset_refault: opt_add(self.workingset_refault, other.workingset_refault),
            workingset_activate: opt_add(self.workingset_activate, other.workingset_activate),
            workingset_nodereclaim: opt_add(
                self.workingset_nodereclaim,
                other.workingset_nodereclaim,
            ),
            pgrefill: opt_add(self.pgrefill, other.pgrefill),
            pgscan: opt_add(self.pgscan, other.pgscan),
            pgsteal: opt_add(self.pgsteal, other.pgsteal),
            pgactivate: opt_add(self.pgactivate, other.pgactivate),
            pgdeactivate: opt_add(self.pgdeactivate, other.pgdeactivate),
            pglazyfree: opt_add(self.pglazyfree, other.pglazyfree),
            pglazyfreed: opt_add(self.pglazyfreed, other.pglazyfreed),
            thp_fault_alloc: opt_add(self.thp_fault_alloc, other.thp_fault_alloc),
            thp_collapse_alloc: opt_add(self.thp_collapse_alloc, other.thp_collapse_alloc),
            memory_high: None,
            events_low: opt_add(self.events_low, other.events_low),
            events_high: opt_add(self.events_high, other.events_high),
            events_max: opt_add(self.events_max, other.events_max),
            events_oom: opt_add(self.events_oom, other.events_oom),
            events_oom_kill: opt_add(self.events_oom_kill, other.events_oom_kill),
        }
    }
}

impl CgroupMemoryModel {
    pub fn new(
        sample: &CgroupSample,
        last: Option<(&CgroupSample, Duration)>,
    ) -> CgroupMemoryModel {
        let mut model = CgroupMemoryModel {
            total: sample.memory_current.map(|v| v as u64),
            swap: sample.memory_swap_current.map(|v| v as u64),
            memory_high: sample.memory_high,
            ..Default::default()
        };
        if let Some(events) = &sample.memory_events {
            model.events_low = events.low.map(|v| v as u64);
            model.events_high = events.high.map(|v| v as u64);
            model.events_max = events.max.map(|v| v as u64);
            model.events_oom = events.oom.map(|v| v as u64);
            model.events_oom_kill = events.oom_kill.map(|v| v as u64);
        }
        if let Some(stat) = &sample.memory_stat {
            model.anon = stat.anon.map(|v| v as u64);
            model.file = stat.file.map(|v| v as u64);
            model.kernel_stack = stat.kernel_stack.map(|v| v as u64);
            model.slab = stat.slab.map(|v| v as u64);
            model.sock = stat.sock.map(|v| v as u64);
            model.shmem = stat.shmem.map(|v| v as u64);
            model.file_mapped = stat.file_mapped.map(|v| v as u64);
            model.file_dirty = stat.file_dirty.map(|v| v as u64);
            model.file_writeback = stat.file_writeback.map(|v| v as u64);
            model.anon_thp = stat.anon_thp.map(|v| v as u64);
            model.inactive_anon = stat.inactive_anon.map(|v| v as u64);
            model.active_anon = stat.active_anon.map(|v| v as u64);
            model.inactive_file = stat.inactive_file.map(|v| v as u64);
            model.active_file = stat.active_file.map(|v| v as u64);
            model.unevictable = stat.unevictable.map(|v| v as u64);
            model.slab_reclaimable = stat.slab_reclaimable.map(|v| v as u64);
            model.slab_unreclaimable = stat.slab_unreclaimable.map(|v| v as u64);

            if let Some((
                CgroupSample {
                    memory_stat: Some(last_stat),
                    ..
                },
                delta,
            )) = last
            {
                model.pgfault = count_per_sec!(last_stat.pgfault, stat.pgfault, delta, u64);
                model.pgmajfault =
                    count_per_sec!(last_stat.pgmajfault, stat.pgmajfault, delta, u64);
                model.workingset_refault = count_per_sec!(
                    last_stat.workingset_refault,
                    stat.workingset_refault,
                    delta,
                    u64
                );
                model.workingset_activate = count_per_sec!(
                    last_stat.workingset_activate,
                    stat.workingset_activate,
                    delta,
                    u64
                );
                model.workingset_nodereclaim = count_per_sec!(
                    last_stat.workingset_nodereclaim,
                    stat.workingset_nodereclaim,
                    delta,
                    u64
                );
                model.pgrefill = count_per_sec!(last_stat.pgrefill, stat.pgrefill, delta, u64);
                model.pgscan = count_per_sec!(last_stat.pgscan, stat.pgscan, delta, u64);
                model.pgsteal = count_per_sec!(last_stat.pgsteal, stat.pgsteal, delta, u64);
                model.pgactivate =
                    count_per_sec!(last_stat.pgactivate, stat.pgactivate, delta, u64);
                model.pgdeactivate =
                    count_per_sec!(last_stat.pgdeactivate, stat.pgdeactivate, delta, u64);
                model.pglazyfree =
                    count_per_sec!(last_stat.pglazyfree, stat.pglazyfree, delta, u64);
                model.pglazyfreed =
                    count_per_sec!(last_stat.pglazyfreed, stat.pglazyfreed, delta, u64);
                model.thp_fault_alloc =
                    count_per_sec!(last_stat.thp_fault_alloc, stat.thp_fault_alloc, delta, u64);
                model.thp_collapse_alloc = count_per_sec!(
                    last_stat.thp_collapse_alloc,
                    stat.thp_collapse_alloc,
                    delta,
                    u64
                );
            }
        }

        model
    }
}

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    below_derive::Queriable
)]
pub struct CgroupPressureModel {
    pub cpu_some_pct: Option<f64>,
    pub io_some_pct: Option<f64>,
    pub io_full_pct: Option<f64>,
    pub memory_some_pct: Option<f64>,
    pub memory_full_pct: Option<f64>,
}

impl CgroupPressureModel {
    fn new(pressure: &cgroupfs::Pressure) -> CgroupPressureModel {
        // Use avg10 instead of calculating pressure with the total metric. If
        // elapsed time between reading pressure total and recording time is too
        // long, pressure could exceed 100%.
        CgroupPressureModel {
            cpu_some_pct: pressure.cpu.some.avg10,
            io_some_pct: pressure.io.some.avg10,
            io_full_pct: pressure.io.full.avg10,
            memory_some_pct: pressure.memory.some.avg10,
            memory_full_pct: pressure.memory.full.avg10,
        }
    }
}
