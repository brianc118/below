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

#[derive(Default, Serialize, Deserialize)]
pub struct ProcessModel {
    pub processes: BTreeMap<i32, SingleProcessModel>,
}

impl ProcessModel {
    pub fn new(sample: &procfs::PidMap, last: Option<(&procfs::PidMap, Duration)>) -> ProcessModel {
        let mut processes: BTreeMap<i32, SingleProcessModel> = BTreeMap::new();

        for (pid, pidinfo) in sample.iter() {
            processes.insert(
                *pid,
                SingleProcessModel::new(
                    &pidinfo,
                    last.and_then(|(p, d)| p.get(pid).map(|p| (p, d))),
                ),
            );
        }

        ProcessModel { processes }
    }
}

#[derive(Default, Serialize, Deserialize, below_derive::Queriable)]
pub struct SingleProcessModel {
    pub pid: Option<i32>,
    pub ppid: Option<i32>,
    pub comm: Option<String>,
    pub state: Option<procfs::PidState>,
    pub uptime_secs: Option<u64>,
    pub cgroup: Option<String>,
    #[queriable(subquery)]
    pub io: Option<ProcessIoModel>,
    #[queriable(subquery)]
    pub mem: Option<ProcessMemoryModel>,
    #[queriable(subquery)]
    pub cpu: Option<ProcessCpuModel>,
    pub cmdline: Option<String>,
    pub exe_path: Option<String>,
}

impl SingleProcessModel {
    fn new(
        sample: &procfs::PidInfo,
        last: Option<(&procfs::PidInfo, Duration)>,
    ) -> SingleProcessModel {
        SingleProcessModel {
            pid: sample.stat.pid,
            ppid: sample.stat.ppid,
            comm: sample.stat.comm.clone(),
            state: sample.stat.state.clone(),
            uptime_secs: sample.stat.running_secs.map(|s| s as u64),
            cgroup: Some(sample.cgroup.clone()),
            io: last.map(|(l, d)| ProcessIoModel::new(&l.io, &sample.io, d)),
            mem: last.map(|(l, d)| ProcessMemoryModel::new(&l, &sample, d)),
            cpu: last.map(|(l, d)| ProcessCpuModel::new(&l.stat, &sample.stat, d)),
            cmdline: if let Some(cmd_vec) = sample.cmdline_vec.as_ref() {
                Some(cmd_vec.join(" "))
            } else {
                Some("?".into())
            },
            exe_path: sample.exe_path.clone(),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, below_derive::Queriable)]
pub struct ProcessIoModel {
    pub rbytes_per_sec: Option<f64>,
    pub wbytes_per_sec: Option<f64>,
    pub rwbytes_per_sec: Option<f64>,
}

impl ProcessIoModel {
    fn new(begin: &procfs::PidIo, end: &procfs::PidIo, delta: Duration) -> ProcessIoModel {
        let rbytes_per_sec = count_per_sec!(begin.rbytes, end.rbytes, delta);
        let wbytes_per_sec = count_per_sec!(begin.wbytes, end.wbytes, delta);
        let rwbytes_per_sec = Some(
            rbytes_per_sec.clone().unwrap_or_default() + wbytes_per_sec.clone().unwrap_or_default(),
        );
        ProcessIoModel {
            rbytes_per_sec,
            wbytes_per_sec,
            rwbytes_per_sec,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, below_derive::Queriable)]
pub struct ProcessCpuModel {
    pub usage_pct: Option<f64>,
    pub user_pct: Option<f64>,
    pub system_pct: Option<f64>,
    pub num_threads: Option<u64>,
}

impl ProcessCpuModel {
    fn new(begin: &procfs::PidStat, end: &procfs::PidStat, delta: Duration) -> ProcessCpuModel {
        let user_pct = usec_pct!(begin.user_usecs, end.user_usecs, delta);
        let system_pct = usec_pct!(begin.system_usecs, end.system_usecs, delta);
        let usage_pct = collector::opt_add(user_pct.clone(), system_pct.clone());
        ProcessCpuModel {
            usage_pct,
            user_pct,
            system_pct,
            num_threads: end.num_threads.map(|t| t as u64),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, below_derive::Queriable)]
pub struct ProcessMemoryModel {
    pub minorfaults_per_sec: Option<f64>,
    pub majorfaults_per_sec: Option<f64>,
    pub rss_bytes: Option<u64>,
    pub vm_size: Option<u64>,
    pub lock: Option<u64>,
    pub pin: Option<u64>,
    pub anon: Option<u64>,
    pub file: Option<u64>,
    pub shmem: Option<u64>,
    pub pte: Option<u64>,
    pub swap: Option<u64>,
    pub huge_tlb: Option<u64>,
}

impl ProcessMemoryModel {
    fn new(begin: &procfs::PidInfo, end: &procfs::PidInfo, delta: Duration) -> ProcessMemoryModel {
        ProcessMemoryModel {
            minorfaults_per_sec: count_per_sec!(begin.stat.minflt, end.stat.minflt, delta),
            majorfaults_per_sec: count_per_sec!(begin.stat.majflt, end.stat.majflt, delta),
            rss_bytes: end.stat.rss_bytes.map(|i| i as u64),
            vm_size: end.mem.vm_size.map(|i| i as u64),
            lock: end.mem.lock.map(|i| i as u64),
            pin: end.mem.pin.map(|i| i as u64),
            anon: end.mem.anon.map(|i| i as u64),
            file: end.mem.file.map(|i| i as u64),
            shmem: end.mem.shmem.map(|i| i as u64),
            pte: end.mem.pte.map(|i| i as u64),
            swap: end.mem.swap.map(|i| i as u64),
            huge_tlb: end.mem.huge_tlb.map(|i| i as u64),
        }
    }
}
