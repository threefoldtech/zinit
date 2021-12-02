use crate::zinit::State;
use crate::zinit::ZInitService;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
pub const DUMMY_ROOT: &str = "";
pub struct ProcessDAG {
    pub adj: HashMap<String, Vec<String>>,
    pub indegree: HashMap<String, u32>,
}
pub async fn service_dependency_order(
    services: &tokio::sync::RwLockReadGuard<'_, HashMap<String, Arc<RwLock<ZInitService>>>>,
) -> ProcessDAG {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut indegree: HashMap<String, u32> = HashMap::new();
    for (name, service) in services.iter() {
        let service = service.read().await;
        for child in service.service.after.iter() {
            children
                .entry(name.into())
                .or_insert(Vec::new())
                .push(child.into());
            *indegree.entry(child.into()).or_insert(0) += 1;
        }
    }
    let mut heads: Vec<String> = Vec::new();
    for (name, _) in services.iter() {
        if *indegree.get(name.into()).unwrap_or(&0) == 0 {
            heads.push(name.into());
            // add edges from the dummy root to the heads
            *indegree.entry(name.into()).or_insert(0) += 1;
        }
    }
    children.insert(DUMMY_ROOT.to_string(), heads);
    ProcessDAG {
        adj: children,
        indegree,
    }
}

#[cfg(test)]
mod tests {
    use crate::zinit::config::{Log, Service, Signal};
    use crate::zinit::ord::service_dependency_order;
    use crate::zinit::State;
    use crate::zinit::Table;
    use crate::zinit::ZInitService;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    fn service(after: Vec<String>) -> Service {
        Service {
            exec: "".to_string(),
            test: "".to_string(),
            one_shot: false,
            after: after,
            log: Log::default(),
            signal: Signal::default(),
            env: HashMap::new(),
            dir: "".to_string(),
        }
    }

    #[tokio::test]
    async fn path() {
        let s1 = ZInitService::new(service(vec![]), State::Spawned);
        let s2 = ZInitService::new(service(vec!["s1".to_string()]), State::Spawned);
        let s3 = ZInitService::new(service(vec!["s2".to_string()]), State::Spawned);
        let s4 = ZInitService::new(service(vec!["s3".to_string()]), State::Spawned);
        let services: Arc<RwLock<Table>> = Arc::new(RwLock::new(Table::new()));
        let mut table = services.write().await;

        table.insert("s1".to_string(), Arc::new(RwLock::new(s1)));
        table.insert("s2".to_string(), Arc::new(RwLock::new(s2)));
        table.insert("s3".to_string(), Arc::new(RwLock::new(s3)));
        table.insert("s4".to_string(), Arc::new(RwLock::new(s4)));
        drop(table);
        let table = services.read().await;
        let ord = service_dependency_order(&table).await;
        assert_eq!(ord, vec!["s1", "s2", "s3", "s4"])
    }
}
