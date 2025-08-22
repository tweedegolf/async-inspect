use std::collections::HashMap;

use anyhow::Result;

use ddbug_parser::FileHash;

use async_fn::AsyncFnType;
use task_pool::{TaskPool, TaskPoolValue};

use self::task_pool::HeaderLayout;

pub(crate) mod async_fn;
pub(crate) mod task_pool;
pub(crate) mod ty;

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
    pub(crate) poll_done_addresses: Vec<u64>,

    pub(crate) async_fn_types: Vec<AsyncFnType>,
    pub(crate) task_pools: Vec<TaskPool>,
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

        let header_layout = HeaderLayout::from_ddbug_data(&file_hash)?;

        let mut task_pools = HashMap::new();
        for unit in file.units() {
            for unit_var in unit.variables() {
                if let Some(task_pool) =
                    TaskPool::from_ddbug_var(unit_var, &async_fn_types, &header_layout, &file_hash)
                {
                    task_pools.insert(task_pool.path.clone(), task_pool);
                }
            }
        }
        let mut task_pools = task_pools.into_values().collect::<Vec<_>>();
        task_pools
            .sort_unstable_by_key(|task| std::cmp::Reverse(task.async_fn_type.layout.total_size));

        let poll_done_addresses = find_poll_function_addresses(&file_hash);
        if poll_done_addresses.is_empty() {
            log::warn!(
                "Could't not find the poll function, manualy break the target to update the display"
            );
        }

        Ok(Self {
            poll_done_addresses,
            task_pools,
            async_fn_types,
        })
    }

    pub(crate) fn get_taskpool_value(&self, task_pool: &TaskPool, bytes: &[u8]) -> TaskPoolValue {
        TaskPoolValue::new(task_pool, bytes, &self.async_fn_types)
    }
}

fn find_poll_function_addresses(file_hash: &FileHash) -> Vec<u64> {
    // Searches for a function with the path: embassy_executor::raw::{impl#9}::poll::{closure#0}
    // where #9 can be replaced with anything.
    let poll_function = 'main: {
        for unit in file_hash.file.units() {
            for unit_fn in unit.functions() {
                if let Some(name) = unit_fn.name()
                    && name.contains("{closure")
                    && let Some(linkage_name) = unit_fn.linkage_name()
                    && linkage_name.contains("SyncExecutor")
                    && let Some(namespace) = unit_fn.namespace()
                    && let namespace = namespace_to_path(namespace)
                    && namespace.starts_with("embassy_executor::raw")
                    && namespace.ends_with("poll")
                {
                    if let [range] = unit_fn.ranges() {
                        return vec![range.end - 4];
                    } else if unit_fn.is_inline() {
                        break 'main unit_fn;
                    }
                }
            }
        }
        return Vec::new();
    };

    // Poll function got inlined, search all functions for where it ended up.
    let mut addresses = Vec::new();
    for unit in file_hash.file.units() {
        for unit_fn in unit.functions() {
            let details = unit_fn.details(file_hash);

            for inlined_function in details.inlined_functions() {
                find_function_in_inlined(
                    poll_function,
                    inlined_function,
                    file_hash,
                    &mut addresses,
                );
            }
        }
    }

    return addresses;
}

/// Recursivly look for all locations the given function is inlined into the givven inlined_function.
/// Adds the last address to the found_addresses.
fn find_function_in_inlined(
    function: &ddbug_parser::Function,
    inlined_function: &ddbug_parser::InlinedFunction,
    file_hash: &FileHash,
    found_addresses: &mut Vec<u64>,
) {
    if let Some(resolved_inlined_function) = inlined_function.abstract_origin(file_hash)
        && ddbug_parser::Function::<'_>::cmp_id(
            file_hash,
            resolved_inlined_function,
            file_hash,
            function,
        )
        .is_eq()
    {
        for range in inlined_function.ranges() {
            if range.begin == 0 {
                // Strange bug where every inlined funciton also has a range staring at 0.
                // Just ignoring it here.
                continue;
            }
            found_addresses.push(range.end);
        }
    }

    for inlined_function in inlined_function.inlined_functions() {
        find_function_in_inlined(function, inlined_function, file_hash, found_addresses);
    }
}
