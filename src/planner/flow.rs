// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! 流控：预算感知的分块发送计划。
//!
//! 这里只算“下一块发多少、什么时候查预算”，不做 IO。真正的写入由
//! `backend::Transport` 负责，设备 worker 按 [`ChunkDecision`] 驱动。
//!
//! 规则来自 `flow-control.md`：
//!
//! - 预算小于剩余总量时查 `0x77`（同一帧带上 `0x70`）；
//! - 单块取 `min(预算, MAX_CHUNK)`，块边界落在命令边界上，不切一条命令；
//! - 预算不足 `max_chunk` 时块间等 `chunk_gap`；
//! - `0x77` 超时按 `fallback_budget` 保守推进；
//! - 整任务超过 `job_timeout` 报 [`FlowError::Timeout`]。

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use thiserror::Error;

use crate::{
    command::{
        req::{PackReqError, ReqTrait},
        resp::buffer_size::BufferSize,
    },
    planner::Assembler,
};

/// 单块最大字节数。预算再大也按这个切，避免一次灌满设备缓冲。
pub const MAX_CHUNK: usize = 9000;
/// 预算不足时的块间等待。
pub const CHUNK_GAP: Duration = Duration::from_millis(50);
/// 等 `0x77` 响应的超时。
pub const BUDGET_TIMEOUT: Duration = Duration::from_secs(5);
/// 单个打印任务的超时。
pub const JOB_TIMEOUT: Duration = Duration::from_secs(60);
/// `0x77` 不响应时按这个字节数保守推进。
pub const FALLBACK_BUDGET: usize = 8 * 1024;

/// 流控参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowConfig {
    /// 单块最大字节数。
    pub max_chunk: usize,
    /// 预算不足时的块间等待。
    pub chunk_gap: Duration,
    /// 等 `0x77` 响应的超时。
    pub budget_timeout: Duration,
    /// 整任务超时。
    pub job_timeout: Duration,
    /// `0x77` 超时后按多少字节保守推进。
    pub fallback_budget: usize,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            max_chunk: MAX_CHUNK,
            chunk_gap: CHUNK_GAP,
            budget_timeout: BUDGET_TIMEOUT,
            job_timeout: JOB_TIMEOUT,
            fallback_budget: FALLBACK_BUDGET,
        }
    }
}

/// 发送循环的下一步。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkDecision {
    /// 把这段命令区写出去。写完后检查 [`FlowPlanner::needs_gap`]。
    Send(Vec<u8>),
    /// 预算不足：先等 [`FlowConfig::chunk_gap`]，再发 `0x70` + `0x77`。
    QueryBudget,
    /// 已经发过预算查询，等 `0x77` 回包后调 [`FlowPlanner::on_buffer_size`]。
    ///
    /// 这个状态用来防止在响应到达前重复查询（SDK 里会自激）。
    Wait,
    /// 队列空了。
    Done,
}

/// 流控错误。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FlowError {
    #[error("print job exceeded its {0:?} deadline")]
    Timeout(Duration),
}

/// 一次发送任务的统计，可用于和 `0x73` 的计数器对账。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlowReport {
    /// 发出的块数。
    pub chunks: usize,
    /// 发出的字节数。
    pub bytes: usize,
    /// 查询预算的次数。
    pub budget_queries: usize,
    /// 是否因为 `0x77` 超时退化到固定预算。
    pub degraded: bool,
}

/// 预算感知的分块发送计划。
#[derive(Debug)]
pub struct FlowPlanner {
    config: FlowConfig,
    queue: VecDeque<Vec<u8>>,
    remaining: usize,
    budget: usize,
    querying: bool,
    started: Instant,
    report: FlowReport,
}

impl FlowPlanner {
    /// 用给定参数新建一个计划。
    pub fn new(config: FlowConfig) -> Self {
        Self {
            config,
            queue: VecDeque::new(),
            remaining: 0,
            budget: 0,
            querying: false,
            started: Instant::now(),
            report: FlowReport::default(),
        }
    }

    /// 追加一段已编码的命令区。空切片会被忽略。
    pub fn push(&mut self, cmd_area: Vec<u8>) {
        if cmd_area.is_empty() {
            return;
        }
        self.remaining += cmd_area.len();
        self.queue.push_back(cmd_area);
    }

    /// 逐条编码并追加。这样块边界天然落在命令边界上。
    pub fn push_commands(
        &mut self,
        assembler: &Assembler,
        cmds: &[&dyn ReqTrait],
    ) -> Result<(), PackReqError> {
        for cmd in cmds {
            let cmd_area = assembler.encode(&[*cmd])?;
            self.push(cmd_area);
        }
        Ok(())
    }

    /// 取下一步动作。
    pub fn step(&mut self) -> ChunkDecision {
        if self.queue.is_empty() {
            return ChunkDecision::Done;
        }
        if self.querying {
            return ChunkDecision::Wait;
        }

        // 退化模式：`0x77` 已经不指望了，每块按保守预算发，块间靠 gap 节流。
        if self.report.degraded {
            let want = self.config.fallback_budget.min(self.config.max_chunk);
            let chunk = self.take_chunk(want);
            self.record_chunk(&chunk);
            return ChunkDecision::Send(chunk);
        }

        // SDK 的条件是「预算 < 剩余总量」，不是「预算 == 0」。
        if self.budget < self.remaining {
            self.querying = true;
            self.report.budget_queries += 1;
            return ChunkDecision::QueryBudget;
        }

        let want = self.budget.min(self.config.max_chunk);
        let chunk = self.take_chunk(want);
        self.budget = self.budget.saturating_sub(chunk.len());
        self.record_chunk(&chunk);
        ChunkDecision::Send(chunk)
    }

    /// 从队首取不超过 `want` 字节的命令，至少取一条。
    fn take_chunk(&mut self, want: usize) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(want.min(self.remaining));
        while let Some(front) = self.queue.front() {
            // 块内至少放一条命令；单条命令超过 `want` 时整条发，不切开。
            if !chunk.is_empty() && chunk.len() + front.len() > want {
                break;
            }
            let cmd = self.queue.pop_front().expect("queue is not empty");
            self.remaining -= cmd.len();
            chunk.extend_from_slice(&cmd);
            if chunk.len() >= want {
                break;
            }
        }
        chunk
    }

    fn record_chunk(&mut self, chunk: &[u8]) {
        self.report.chunks += 1;
        self.report.bytes += chunk.len();
    }

    /// 收到 `0x77` 回包后更新预算。
    pub fn on_buffer_size(&mut self, resp: &BufferSize) {
        self.budget = resp.budget();
        self.querying = false;
    }

    /// `0x77` 超时：按保守预算继续，不让整页卡死。
    pub fn on_budget_timeout(&mut self) {
        self.budget = self.config.fallback_budget;
        self.querying = false;
        self.report.degraded = true;
    }

    /// 发出 [`ChunkDecision::Send`] 之后，是否需要先等一会。
    ///
    /// SDK 在「预算不足 `max_chunk` 且还有数据」时等 `chunk_gap`，让打印头
    /// 消耗掉一块，而不是等它真的打印完。
    pub fn needs_gap(&self) -> bool {
        !self.queue.is_empty() && (self.report.degraded || self.budget < self.config.max_chunk)
    }

    /// 是否已经全部发完。
    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }

    /// 检查整任务超时。
    pub fn check_deadline(&self) -> Result<(), FlowError> {
        let elapsed = self.started.elapsed();
        if elapsed > self.config.job_timeout {
            Err(FlowError::Timeout(elapsed))
        } else {
            Ok(())
        }
    }

    /// 当前统计。
    pub fn report(&self) -> FlowReport {
        self.report
    }

    /// 当前参数。
    pub fn config(&self) -> &FlowConfig {
        &self.config
    }
}

impl Default for FlowPlanner {
    fn default() -> Self {
        Self::new(FlowConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planner() -> FlowPlanner {
        FlowPlanner::default()
    }

    fn buffer(size: u16) -> BufferSize {
        BufferSize {
            size,
            status: 0,
            sync_token: 0,
            tail: Vec::new(),
        }
    }

    fn drain(p: &mut FlowPlanner) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            match p.step() {
                ChunkDecision::Send(chunk) => out.push(chunk),
                ChunkDecision::Done => return out,
                other => panic!("unexpected decision: {other:?}"),
            }
        }
    }

    #[test]
    fn empty_queue_is_done() {
        assert!(matches!(planner().step(), ChunkDecision::Done));
    }

    #[test]
    fn sends_min_of_budget_and_max_chunk() {
        let mut p = planner();
        for _ in 0..20 {
            p.push(vec![0u8; 1000]);
        }
        p.on_buffer_size(&buffer(100)); // 50000
        let chunks = drain(&mut p);
        assert_eq!(
            chunks.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![9000, 9000, 2000]
        );
        assert_eq!(p.report().chunks, 3);
        assert_eq!(p.report().bytes, 20_000);
        assert!(p.is_done());
    }

    #[test]
    fn asks_for_budget_when_budget_is_less_than_pending() {
        let mut p = planner();
        for _ in 0..20 {
            p.push(vec![0u8; 1000]);
        }
        p.on_buffer_size(&buffer(10)); // 5000 < 20000
        assert!(matches!(p.step(), ChunkDecision::QueryBudget));
        // 响应到达前不重复查询。
        assert!(matches!(p.step(), ChunkDecision::Wait));
        assert!(matches!(p.step(), ChunkDecision::Wait));

        p.on_buffer_size(&buffer(100));
        assert!(matches!(p.step(), ChunkDecision::Send(c) if c.len() == 9000));
        assert_eq!(p.report().budget_queries, 1);
    }

    #[test]
    fn never_splits_a_single_command() {
        let mut p = planner();
        p.push(vec![0u8; 12_000]);
        p.on_buffer_size(&buffer(100));
        assert!(matches!(p.step(), ChunkDecision::Send(c) if c.len() == 12_000));
        assert!(matches!(p.step(), ChunkDecision::Done));
    }

    #[test]
    fn timeout_falls_back_to_eight_kib() {
        let mut p = planner();
        for _ in 0..10 {
            p.push(vec![0u8; 1024]);
        }
        p.on_budget_timeout();
        assert!(p.report().degraded);
        assert!(matches!(p.step(), ChunkDecision::Send(c) if c.len() == 8192));
    }

    #[test]
    fn gap_is_needed_only_when_budget_is_short_and_data_remains() {
        let mut p = planner();
        assert!(!p.needs_gap());

        p.push(vec![0u8; 1000]);
        p.on_buffer_size(&buffer(100));
        assert!(!p.needs_gap(), "50000 >= 9000");

        p.on_buffer_size(&buffer(4)); // 2000
        assert!(p.needs_gap(), "2000 < 9000 and data remains");

        let _ = p.step();
        assert!(p.is_done());
        assert!(!p.needs_gap());
    }

    #[test]
    fn job_deadline_is_reported() {
        let p = FlowPlanner::new(FlowConfig {
            job_timeout: Duration::from_millis(1),
            ..Default::default()
        });
        std::thread::sleep(Duration::from_millis(2));
        assert!(matches!(p.check_deadline(), Err(FlowError::Timeout(_))));
    }

    #[test]
    fn push_commands_uses_command_boundaries() {
        let assembler = Assembler::new(crate::command::ModelVersion::V2);
        let mut p = planner();
        let cmds: Vec<&dyn ReqTrait> = vec![
            &crate::command::req::status::Status {},
            &crate::command::req::buffer_size::BufferSize {},
        ];
        p.push_commands(&assembler, &cmds).unwrap();
        p.on_buffer_size(&buffer(100));
        let chunks = drain(&mut p);
        assert_eq!(chunks.len(), 1);
        // 两条命令编码后拼成一块，校验和在每条命令内部。
        assert_eq!(chunks[0][0..2], [0x1F, 0x70]);
        assert_eq!(chunks[0][4..6], [0x1F, 0x77]);
    }
}
