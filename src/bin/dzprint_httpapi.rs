use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
    routing, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    main_fn().await
}

async fn main_fn() -> anyhow::Result<()> {
    Ok(())
}

async fn http_server() -> anyhow::Result<()> {
    let state = Arc::new(ApiState {
        printer: PrinterState::new().await,
    });
    let app = Router::new()
        .route(
            "/dzprint/add_print_job",
            routing::post(add_print_job_handler),
        )
        .route(
            "/dzprint/get_print_job_status",
            routing::post(get_print_job_status_handler),
        )
        .route(
            "/dzprint/cancel_print_job",
            routing::post(cancel_print_job_handler),
        )
        .route(
            "/dzprint/get_printer_status",
            routing::post(get_printer_status_handler),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3333));

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

#[axum::debug_handler]
pub async fn add_print_job_handler(
    State(state): State<S>,
    req: Json<AddPrintJobRequest>,
) -> R<AddPrintJobResponse> {
    Err(ErrorCode::InternalError)
}

#[axum::debug_handler]
pub async fn get_print_job_status_handler(
    State(state): State<S>,
    req: Json<GetPrintJobStatusRequest>,
) -> R<GetPrintJobStatusResponse> {
    Err(ErrorCode::InternalError)
}

#[axum::debug_handler]
pub async fn cancel_print_job_handler(
    State(state): State<S>,
    req: Json<CancelPrintJobRequest>,
) -> R<CancelPrintJobResponse> {
    Err(ErrorCode::InternalError)
}

#[axum::debug_handler]
pub async fn get_printer_status_handler(
    State(state): State<S>,
    req: Json<GetPrinterStatusRequest>,
) -> R<GetPrinterStatusResponse> {
    Err(ErrorCode::InternalError)
}

pub type S = Arc<ApiState>;

#[derive(Debug, thiserror::Error)]
pub enum ErrorCode {
    // #[error("Incorrect Server Key")]
    // IncorrectServerKey,
    #[error("Tokio Join Error")]
    TokioJoinError(#[from] tokio::task::JoinError),
    #[error("Internal Error")]
    InternalError,
    #[error("Job not found")]
    JobNotFound,
    #[error("Job completed")]
    JobCompleted,
    // #[error("MsgPack Encode Error: {0:?}")]
    // MsgpackEncodeError(#[from] rmp_serde::encode::Error),
    // #[error("MsgPack Decode Error: {0:?}")]
    // MsgpackDecodeError(#[from] rmp_serde::decode::Error),
    // #[error("Crypto Message Error")]
    // CryptoMessageError(#[from] crypto::message::Error),
}

impl IntoResponse for ErrorCode {
    fn into_response(self) -> Response {
        (
            axum::http::StatusCode::FORBIDDEN,
            axum::Json::from(ErrorResponse { error: self }),
        )
            .into_response()
    }
}

impl Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}", self))
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorCode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessResponse<X>
where
    X: Serialize,
{
    pub data: X,
}

pub type R<T> = Result<Json<SuccessResponse<T>>, ErrorCode>;

pub struct ApiState {
    pub printer: PrinterState,
}

#[derive(Debug)]
pub struct PrinterState {}

impl PrinterState {
    pub async fn new() -> Self {
        Self {}
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ApiPaperType {
    /// 小票纸
    Ticket,
    /// 透明贴
    LocatorHole,
    /// 不干胶
    Adhesive,
    /// 卡纸
    CardPaper,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ApiPrintJobStatusType {
    Pending,
    Printing,
    Errored,
    Canceled,
    Completed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ApiPrinterStatusType {
    WaitDevice,
    Idle,
    Printing,
    Errored,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddPrintJobRequest {
    pub bitmap: String,
    pub paper: ApiPaperType,
    pub darkness: i8,
    pub speed: i8,
    pub gap: i16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AddPrintJobResponse {
    pub job_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetPrintJobStatusRequest {
    pub job_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetPrintJobStatusResponse {
    pub status: ApiPrintJobStatusType,
    pub command_all: i64,
    pub command_executed: i64,
    pub paper: ApiPaperType,
    pub darkness: i8,
    pub speed: i8,
    pub gap: i16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CancelPrintJobRequest {
    pub job_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CancelPrintJobResponse {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetPrinterStatusRequest {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetPrinterStatusResponse {
    pub jobs: Vec<String>,
    pub status: ApiPrinterStatusType,
    pub model: String,
}
