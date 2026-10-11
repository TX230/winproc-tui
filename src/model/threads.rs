#[derive(Debug, Clone)]
pub(crate) struct ThreadRow {
    pub(crate) tid: u32,
    pub(crate) name: Option<String>,
    pub(crate) cpu_seconds: Option<f64>,
    pub(crate) base_priority: i32,
    pub(crate) ideal_cpu: Option<(u16, u8)>,
}
#[derive(Debug, Clone)]
pub(crate) struct ThreadReport {
    pub(crate) rows: Vec<ThreadRow>,
    pub(crate) unavailable: usize,
}
