// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! gRPC 接口：只做校验、落临时文件、写任务表、入队。

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use mytm::TaskManager;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use dz_print::command::status::PrinterStatus as LibPrinterStatus;

use crate::{
    dzsrv::{
        JobCancel, JobHandle, JobStatus, PrintImageReq, PrintTypstReq, PrinterStatus,
        Status as PrintStatus, mgmt_server::Mgmt, print_server::Print,
    },
    job::{JobId, JobSnapshot, JobState, JobStore},
    render::JobSpec,
    render::run_blocking,
};

#[derive(Clone)]
pub(crate) struct ApiState {
    store: JobStore,
    ingress: mpsc::Sender<JobSpec>,
    root: CancellationToken,
    tm: TaskManager,
    temp_root: PathBuf,
    accepting: Arc<AtomicBool>,
    max_job_bytes: usize,
    max_repeats: u32,
}

impl ApiState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: JobStore,
        ingress: mpsc::Sender<JobSpec>,
        root: CancellationToken,
        tm: TaskManager,
        temp_root: PathBuf,
        accepting: Arc<AtomicBool>,
        max_job_bytes: usize,
        max_repeats: u32,
    ) -> Self {
        Self {
            store,
            ingress,
            root,
            tm,
            temp_root,
            accepting,
            max_job_bytes,
            max_repeats,
        }
    }

    fn new_job(&self) -> (JobId, CancellationToken) {
        let id = *Uuid::new_v4().as_bytes();
        (id, self.root.child_token())
    }

    fn repeats(&self, repeats: u32) -> Result<u32, Status> {
        if repeats > self.max_repeats {
            return Err(Status::invalid_argument(format!(
                "repeats 最大为 {}",
                self.max_repeats
            )));
        }
        Ok(repeats)
    }

    async fn prepare(
        &self,
        name: &'static str,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<(TempDir, PathBuf), Status> {
        if bytes.len() > self.max_job_bytes {
            return Err(Status::resource_exhausted(format!(
                "请求体 {} 字节，超过上限 {}",
                bytes.len(),
                self.max_job_bytes
            )));
        }

        let root = self.temp_root.clone();
        run_blocking(&self.tm, cancel, move || {
            let temp = tempfile::Builder::new().prefix("job-").tempdir_in(&root)?;
            let path = temp.path().join(name);
            std::fs::write(&path, &bytes)?;
            Ok((temp, path))
        })
        .await
        .map_err(|error| Status::internal(format!("准备临时文件失败: {error:#}")))
    }

    fn submit(&self, spec: JobSpec) -> Result<(), Status> {
        if !self.accepting.load(Ordering::SeqCst) {
            return Err(Status::unavailable("server is shutting down"));
        }
        match self.ingress.try_send(spec) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                Err(Status::resource_exhausted("打印队列已满"))
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                Err(Status::unavailable("server is shutting down"))
            }
        }
    }
}

pub struct PrintEndpoint {
    state: ApiState,
}

impl PrintEndpoint {
    pub fn new(state: ApiState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl Print for PrintEndpoint {
    async fn print_image(
        &self,
        request: Request<PrintImageReq>,
    ) -> Result<Response<JobHandle>, Status> {
        let request = request.into_inner();
        if request.image_file.is_empty() {
            return Err(Status::invalid_argument("image_file 不能为空"));
        }
        let repeats = self.state.repeats(request.repeats)?;
        let (id, cancel) = self.state.new_job();
        let (temp, input) = self
            .state
            .prepare("input.img", request.image_file, &cancel)
            .await?;

        self.state.store.insert(id, cancel.clone());
        let spec = JobSpec::Image {
            id,
            temp,
            input,
            repeats,
            cancel,
        };
        if let Err(status) = self.state.submit(spec) {
            self.state.store.remove(&id);
            return Err(status);
        }

        Ok(Response::new(JobHandle {
            job_id: id.to_vec(),
        }))
    }

    async fn print_typst(
        &self,
        request: Request<PrintTypstReq>,
    ) -> Result<Response<JobHandle>, Status> {
        let request = request.into_inner();
        if request.typst_tar.is_empty() {
            return Err(Status::invalid_argument("typst_tar 不能为空"));
        }
        let repeats = self.state.repeats(request.repeats)?;
        let (id, cancel) = self.state.new_job();
        let (temp, tar) = self
            .state
            .prepare("input.tar", request.typst_tar, &cancel)
            .await?;

        self.state.store.insert(id, cancel.clone());
        let spec = JobSpec::Typst {
            id,
            temp,
            tar,
            repeats,
            cancel,
        };
        if let Err(status) = self.state.submit(spec) {
            self.state.store.remove(&id);
            return Err(status);
        }

        Ok(Response::new(JobHandle {
            job_id: id.to_vec(),
        }))
    }
}

pub struct MgmtEndpoint {
    state: ApiState,
}

impl MgmtEndpoint {
    pub fn new(state: ApiState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl Mgmt for MgmtEndpoint {
    async fn status(&self, request: Request<JobHandle>) -> Result<Response<JobStatus>, Status> {
        let Some(id) = parse_id(&request.into_inner().job_id) else {
            return Ok(Response::new(empty_status()));
        };
        let Some(snapshot) = self.state.store.snapshot(&id) else {
            return Ok(Response::new(empty_status()));
        };

        Ok(Response::new(JobStatus {
            exists: true,
            job_status: job_state(&snapshot) as i32,
            printer_status: printer_status(&snapshot) as i32,
        }))
    }

    async fn cancel(&self, request: Request<JobHandle>) -> Result<Response<JobCancel>, Status> {
        let Some(id) = parse_id(&request.into_inner().job_id) else {
            return Ok(Response::new(JobCancel { cancelled: false }));
        };
        let cancelled = self.state.store.cancel(&id);
        Ok(Response::new(JobCancel { cancelled }))
    }
}

fn parse_id(bytes: &[u8]) -> Option<JobId> {
    bytes.try_into().ok()
}

fn empty_status() -> JobStatus {
    JobStatus {
        exists: false,
        job_status: PrintStatus::InQueue as i32,
        printer_status: PrinterStatus::PsNotCompleted as i32,
    }
}

fn job_state(snapshot: &JobSnapshot) -> PrintStatus {
    match snapshot.state {
        JobState::InQueue => PrintStatus::InQueue,
        JobState::Rendering => PrintStatus::Rendering,
        JobState::RenderFailed => PrintStatus::RenderFailed,
        JobState::PrintPending => PrintStatus::PrintPending,
        JobState::Printing => PrintStatus::Printing,
        JobState::PrintCompleted => PrintStatus::PrintCompleted,
        JobState::Cancelled => PrintStatus::Cancelled,
    }
}

fn printer_status(snapshot: &JobSnapshot) -> PrinterStatus {
    if snapshot.state == JobState::Cancelled {
        return PrinterStatus::PsJobCanceled;
    }
    if snapshot.io_error {
        return PrinterStatus::PsIoError;
    }
    match snapshot.printer_status {
        Some(status) => convert_printer_status(status),
        None => match snapshot.state {
            JobState::PrintCompleted => PrinterStatus::PsPrintable,
            _ => PrinterStatus::PsNotCompleted,
        },
    }
}

fn convert_printer_status(status: LibPrinterStatus) -> PrinterStatus {
    match status {
        LibPrinterStatus::Printable => PrinterStatus::PsPrintable,
        LibPrinterStatus::Printing => PrinterStatus::PsPrinting,
        LibPrinterStatus::Rotating => PrinterStatus::PsRotating,
        LibPrinterStatus::NoJob => PrinterStatus::PsNoJob,
        LibPrinterStatus::PageNotReady => PrinterStatus::PsPageNotReady,
        LibPrinterStatus::JobCanceled => PrinterStatus::PsJobCanceled,
        LibPrinterStatus::EnvNotReady => PrinterStatus::PsEnvNotReady,
        LibPrinterStatus::VolTooLow => PrinterStatus::PsVolTooLow,
        LibPrinterStatus::VolTooHigh => PrinterStatus::PsVolTooHigh,
        LibPrinterStatus::TphNotFound => PrinterStatus::PsTphNotFound,
        LibPrinterStatus::TphTooHot => PrinterStatus::PsTphTooHot,
        LibPrinterStatus::CoverOpened => PrinterStatus::PsCoverOpened,
        LibPrinterStatus::NoPaper => PrinterStatus::PsNoPaper,
        LibPrinterStatus::RibbonCanOpened => PrinterStatus::PsRibbonCanOpened,
        LibPrinterStatus::NoRibbon => PrinterStatus::PsNoRibbon,
        LibPrinterStatus::UnmatchedRibbon => PrinterStatus::PsUnmatchedRibbon,
        LibPrinterStatus::TphTooCold => PrinterStatus::PsTphTooCold,
        LibPrinterStatus::UsedupRibbon => PrinterStatus::PsUsedupRibbon,
        LibPrinterStatus::UsedupRibbon2 => PrinterStatus::PsUsedupRibbon2,
        LibPrinterStatus::NoLabel => PrinterStatus::PsNoLabel,
        LibPrinterStatus::UnmatchedLabel => PrinterStatus::PsUnmatchedLabel,
        LibPrinterStatus::UsedupLabel => PrinterStatus::PsUsedupLabel,
        LibPrinterStatus::NoRibbon2 => PrinterStatus::PsNoRibbon2,
        LibPrinterStatus::UnmatchedRibbon2 => PrinterStatus::PsUnmatchedRibbon2,
        LibPrinterStatus::LabelCanOpened => PrinterStatus::PsLabelCanOpened,
        LibPrinterStatus::Unknown(_) => PrinterStatus::PsOtherError,
    }
}
