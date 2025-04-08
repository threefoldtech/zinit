use crate::zinit::types::ServiceTable;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
pub const DUMMY_ROOT: &str = "";
pub struct ProcessDAG {
    pub adj: HashMap<String, Vec<String>>,
    pub indegree: HashMap<String, u32>,
    /// number of services including the dummy root
    pub count: u32,
}
pub async fn service_dependency_order(services: Arc<RwLock<ServiceTable>>) -> ProcessDAG {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut indegree: HashMap<String, u32> = HashMap::new();
    let table = services.read().await;
    for (name, service) in table.iter() {
        let service = service.read().await;
        for child in service.service.after.iter() {
            children
                .entry(name.into())
                .or_default()
                .push(child.into());
            *indegree.entry(child.into()).or_insert(0) += 1;
        }
    }
    let mut heads: Vec<String> = Vec::new();
    for (name, _) in table.iter() {
        if *indegree.get::<str>(name).unwrap_or(&0) == 0 {
            heads.push(name.into());
            // add edges from the dummy root to the heads
            *indegree.entry(name.into()).or_insert(0) += 1;
        }
    }
    children.insert(DUMMY_ROOT.to_string(), heads);
    ProcessDAG {
        adj: children,
        indegree,
        count: table.len() as u32 + 1,
    }
}
