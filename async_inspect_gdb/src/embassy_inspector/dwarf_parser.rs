use std::collections::HashMap;

use anyhow::Result;

use async_fn::AsyncFnType;
use ddbug_parser::FileHash;

pub mod async_fn;
pub mod task_pool;

fn namespace_to_path(namespace: &ddbug_parser::Namespace<'_>) -> String {
    let name = namespace.name().unwrap_or("<unknown>");
    match namespace.parent() {
        Some(parent) => {
            let mut path = namespace_to_path(parent);
            path.push_str("::");
            path.push_str(&name);
            path
        }
        None => name.to_owned(),
    }
}

fn from_namespace_and_name(
    namespace: Option<&ddbug_parser::Namespace<'_>>,
    name: Option<&str>,
) -> String {
    let mut result = String::new();
    if let Some(namespace) = namespace {
        result.push_str(&namespace_to_path(namespace));
        result.push_str("::");
    }
    if let Some(type_name) = name {
        result.push_str(type_name);
    } else {
        result.push_str("<unknown>");
    }
    result
}

#[derive(Debug, Clone)]
pub(crate) struct DebugData {
    pub(crate) async_fn_types: Vec<AsyncFnType>,
    pub(crate) task_pools: Vec<task_pool::TaskPool>,
}

impl DebugData {
    pub(crate) fn from_object_file(path: String) -> Result<Self> {
        let file = ddbug_parser::File::parse(path)?;
        let file = file.file();
        let file_hash = FileHash::new(file);

        let mut async_fn_types = HashMap::new();
        for unit in file.units() {
            for unit_type in unit.types() {
                if let Some(future) = AsyncFnType::from_ddbug_type(unit_type, &file_hash)? {
                    async_fn_types.insert(future.path.clone(), future);
                }
            }
        }
        let mut async_fn_types = async_fn_types.into_values().collect::<Vec<_>>();
        async_fn_types.sort_unstable_by(|a, b| b.layout.total_size.cmp(&a.layout.total_size));

        let mut task_pools = HashMap::new();
        for unit in file.units() {
            for unit_var in unit.variables() {
                if let Some(task_pool) =
                    task_pool::TaskPool::from_ddbug_var(unit_var, &async_fn_types, &file_hash)
                {
                    task_pools.insert(task_pool.path.clone(), task_pool);
                }
            }
        }
        let mut task_pools = task_pools.into_values().collect::<Vec<_>>();
        task_pools
            .sort_unstable_by_key(|task| std::cmp::Reverse(task.async_fn_type.layout.total_size));

        Ok(Self {
            task_pools,
            async_fn_types,
        })
    }
}
