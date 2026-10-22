//! Session-only ODBC workers. Workers never hold or mutate a workbook.
use gridcraft_odbc::{Backend, Connection, DataSet, Limits, NativeBackend, OdbcError};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    Ready,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: String,
    pub state: JobState,
    pub kind: String,
    pub error: Option<String>,
    pub rows: Option<usize>,
    pub columns: Option<usize>,
    pub truncated: bool,
}

#[derive(Clone)]
pub struct Request {
    pub uid: u64,
    pub revision: u64,
    pub kind: String,
    pub connection: Connection,
    pub sql: String,
    pub limits: Limits,
    pub table: Option<String>,
    pub connection_ref: Option<String>,
    pub replace: bool,
    pub batch: Vec<Request>,
}

#[derive(Clone)]
pub struct BatchItem {
    pub request: Request,
    pub data: Result<DataSet, String>,
}

enum Outcome {
    Single(Option<DataSet>),
    Batch(Vec<BatchItem>),
}

struct Job {
    info: JobInfo,
    request: Request,
    data: Option<DataSet>,
    batch: Option<Vec<BatchItem>>,
    cancel: Arc<AtomicBool>,
}

pub struct OdbcManager {
    jobs: Arc<Mutex<Vec<Job>>>,
    profiles: HashMap<(u64, String), Connection>,
    backend: Arc<dyn Backend>,
    next_id: u64,
}

impl Default for OdbcManager {
    fn default() -> Self {
        Self::new(Arc::new(NativeBackend))
    }
}

impl OdbcManager {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self { jobs: Arc::new(Mutex::new(Vec::new())), profiles: HashMap::new(), backend, next_id: 1 }
    }

    pub fn sources(&self) -> Result<gridcraft_odbc::Sources, OdbcError> {
        self.backend.sources()
    }

    pub fn submit(&mut self, mut request: Request) -> Result<String, String> {
        if cfg!(target_arch = "wasm32") {
            return Err(OdbcError::Unavailable.to_string());
        }
        request.limits.validate().map_err(|e| e.to_string())?;
        if request.kind == "refreshAll" {
            if request.batch.is_empty() || request.batch.len() > 64 {
                return Err("Refresh All supports at most 64 ODBC queries.".into());
            }
        } else {
            request.connection.validate().map_err(|e| e.to_string())?;
            if !matches!(request.kind.as_str(), "test" | "preview" | "load" | "refresh") || !request.batch.is_empty() {
                return Err("Invalid ODBC operation.".into());
            }
            if request.kind != "test" {
                gridcraft_odbc::validate_query(&request.sql).map_err(|e| e.to_string())?;
            }
        }
        if request.kind == "load" && self.profiles.len() >= 64 {
            return Err("At most 64 ODBC connections may be kept in this session.".into());
        }
        if matches!(request.kind.as_str(), "load" | "refresh") {
            request.connection_ref = Some(connection_reference()?);
        }
        for item in &mut request.batch {
            item.connection_ref = Some(connection_reference()?);
        }
        install_panic_redaction();
        let id = format!("odbc-{}", self.next_id);
        self.next_id = self.next_id.checked_add(1).ok_or("ODBC job identifiers are exhausted.")?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.iter().any(|j| matches!(j.info.state, JobState::Running | JobState::Ready)) {
                return Err("Finish or cancel the current ODBC job first.".into());
            }
            if jobs.len() >= 32 {
                jobs.remove(0);
            }
            jobs.push(Job {
                info: JobInfo {
                    id: id.clone(),
                    state: JobState::Running,
                    kind: request.kind.clone(),
                    error: None,
                    rows: None,
                    columns: None,
                    truncated: false,
                },
                request: request.clone(),
                data: None,
                batch: None,
                cancel: cancel.clone(),
            });
        }
        let jobs = self.jobs.clone();
        let backend = self.backend.clone();
        let worker_id = id.clone();
        let spawn = thread::Builder::new().name("gridcraft-odbc".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(OdbcError::Cancelled);
                }
                if request.kind == "refreshAll" {
                    let mut items = Vec::new();
                    let mut remaining = gridcraft_odbc::MAX_BYTES;
                    for item in &request.batch {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(OdbcError::Cancelled);
                        }
                        let mut item = item.clone();
                        item.limits.bytes = item.limits.bytes.min(remaining);
                        let data = (|| {
                            item.connection.validate()?;
                            gridcraft_odbc::validate_query(&item.sql)?;
                            item.limits.validate()?;
                            let data = backend.query(&item.connection, &item.sql, item.limits, false, &cancel)?;
                            if data.truncated {
                                return Err(OdbcError::Limit("The result exceeds the row limit."));
                            }
                            remaining = remaining.saturating_sub(data.validate(item.limits)?);
                            Ok::<_, OdbcError>(data)
                        })()
                        .map_err(|e| e.to_string());
                        items.push(BatchItem { request: item, data });
                    }
                    Ok(Outcome::Batch(items))
                } else if request.kind == "test" {
                    backend.test(&request.connection).map(|_| Outcome::Single(None))
                } else {
                    let data = backend.query(&request.connection, &request.sql, request.limits, request.kind == "preview", &cancel)?;
                    if request.kind != "preview" && data.truncated {
                        return Err(OdbcError::Limit("The result exceeds the row limit."));
                    }
                    data.validate(request.limits)?;
                    Ok(Outcome::Single(Some(data)))
                }
            }));
            let mut jobs = jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(job) = jobs.iter_mut().find(|j| j.info.id == worker_id) else { return };
            // Keep Running until the driver returns, even when cancellation was requested.
            if cancel.load(Ordering::Relaxed) {
                job.info.state = JobState::Cancelled;
            } else {
                match result {
                    Ok(Ok(Outcome::Single(Some(data)))) => {
                        job.info.rows = Some(data.rows.len());
                        job.info.columns = Some(data.columns.len());
                        job.info.truncated = data.truncated;
                        job.data = Some(data);
                        job.info.state = JobState::Ready;
                    }
                    Ok(Ok(Outcome::Batch(items))) => {
                        job.info.rows = Some(items.iter().filter_map(|item| item.data.as_ref().ok()).map(|d| d.rows.len()).sum());
                        job.batch = Some(items);
                        job.info.state = JobState::Ready;
                    }
                    Ok(Ok(Outcome::Single(None))) => job.info.state = JobState::Completed,
                    Ok(Err(OdbcError::Cancelled)) => job.info.state = JobState::Cancelled,
                    Ok(Err(error)) => {
                        job.info.state = JobState::Failed;
                        job.info.error = Some(error.to_string());
                    }
                    Err(_) => {
                        job.info.state = JobState::Failed;
                        job.info.error = Some("The ODBC driver operation failed.".into());
                    }
                }
            }
            // Credentials remain only in a successful load job until it is applied.
            if job.info.state != JobState::Ready {
                clear_request(&mut job.request);
            }
        });
        if spawn.is_err() {
            self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|j| j.info.id != id);
            return Err("Could not start the ODBC worker.".into());
        }
        Ok(id)
    }

    pub fn status(&self, id: &str) -> Option<JobInfo> {
        self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().find(|j| j.info.id == id).map(|j| j.info.clone())
    }

    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let job = jobs.iter_mut().find(|j| j.info.id == id).ok_or("ODBC job was not found.")?;
        job.cancel.store(true, Ordering::Relaxed);
        if job.info.state == JobState::Ready {
            job.info.state = JobState::Cancelled;
            job.data = None;
            job.batch = None;
            clear_request(&mut job.request);
        }
        Ok(())
    }

    pub fn ready(&self, id: &str, uid: u64, revision: u64) -> Result<(DataSet, Request), String> {
        let jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let job = jobs.iter().find(|j| j.info.id == id).ok_or("ODBC job was not found.")?;
        if job.info.state != JobState::Ready || job.cancel.load(Ordering::Relaxed) {
            return Err("ODBC job is not ready.".into());
        }
        if job.request.uid != uid || job.request.revision != revision {
            return Err("The workbook changed while querying; rerun the query.".into());
        }
        let data = job.data.clone().ok_or("ODBC job has no result.")?;
        Ok((data, job.request.clone()))
    }

    pub fn complete(&mut self, id: &str, applied_tables: &[String]) {
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(job) = jobs.iter_mut().find(|j| j.info.id == id) {
            let requests = if job.request.kind == "refreshAll" { job.request.batch.as_slice() } else { std::slice::from_ref(&job.request) };
            for request in requests.iter().filter(|r| matches!(r.kind.as_str(), "load" | "refresh")) {
                if job.request.kind == "refreshAll" && !request.table.as_ref().is_some_and(|table| applied_tables.contains(table)) {
                    continue;
                }
                let key = (request.uid, request.connection_ref.clone().unwrap_or_else(|| id.into()));
                if self.profiles.len() < 64 || self.profiles.contains_key(&key) {
                    self.profiles.insert(key, request.connection.clone());
                }
            }
            job.info.state = JobState::Completed;
            job.data = None;
            job.batch = None;
            clear_request(&mut job.request);
        }
    }

    pub fn profile(&self, uid: u64, id: &str) -> Option<Connection> {
        self.profiles.get(&(uid, id.into())).cloned()
    }

    pub fn retain_profiles(&mut self, references: &std::collections::HashSet<(u64, String)>) {
        self.profiles.retain(|key, _| references.contains(key));
    }

    pub fn ready_batch(&self, id: &str, uid: u64, revision: u64) -> Result<Vec<BatchItem>, String> {
        let jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let job = jobs.iter().find(|j| j.info.id == id).ok_or("ODBC job was not found.")?;
        if job.info.state != JobState::Ready || job.cancel.load(Ordering::Relaxed) {
            return Err("ODBC job is not ready.".into());
        }
        if job.request.uid != uid || job.request.revision != revision {
            return Err("The workbook changed while querying; rerun Refresh All.".into());
        }
        job.batch.clone().ok_or_else(|| "This is not a Refresh All job.".into())
    }

    pub fn discard_document(&mut self, uid: u64) {
        self.profiles.retain(|(document, _), _| *document != uid);
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for job in jobs.iter_mut().filter(|j| j.request.uid == uid) {
            job.cancel.store(true, Ordering::Relaxed);
            job.data = None;
            job.batch = None;
            if job.info.state != JobState::Running {
                job.info.state = JobState::Cancelled;
            }
            clear_request(&mut job.request);
        }
    }
}

fn clear_request(request: &mut Request) {
    request.connection = Connection::default();
    request.sql.clear();
    request.batch.clear();
}

#[cfg(not(target_arch = "wasm32"))]
fn connection_reference() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Could not create an ODBC connection reference.".to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(target_arch = "wasm32")]
fn connection_reference() -> Result<String, String> {
    Err(OdbcError::Unavailable.to_string())
}

fn install_panic_redaction() {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if thread::current().name() == Some("gridcraft-odbc") {
                let _ = std::io::Write::write_all(&mut std::io::stderr(), b"gridcraft: ODBC driver operation failed\n");
            } else {
                previous(info);
            }
        }));
    });
}
