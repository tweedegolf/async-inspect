//! Models for the memory layout and locations of task pools.

use std::collections::HashMap;

use super::{
    async_fn::{AsyncFnType, AsyncFnValue},
    future::{FutureType, FutureTypeKind, FutureValue},
    namespace_to_path,
    ty::Type,
};

use ddbug_parser::{FileHash, Result, TypeKind};

#[derive(Debug, Clone)]
pub(crate) enum StateType {
    /// State is a single u8, using
    /// - 0b00 for uninit
    /// - 0b01 for spawned
    /// - 0b11 for queued
    U8,
    /// State is a single u32, using
    /// - 0b0 0000 0000 for uninit
    /// - 0b0 0000 0001 for spawned
    /// - 0b1 0000 0000 for queued
    U32,
}

#[derive(Debug, Clone)]
pub(crate) struct HeaderLayout {
    state_offset: u64,
    state_type: StateType,
}

impl HeaderLayout {
    fn from_ddbug_type(
        ddbug_type: &ddbug_parser::Type<'_>,
        file_hash: &&FileHash<'_>,
    ) -> Result<Option<Self>> {
        let TypeKind::Struct(struct_type) = ddbug_type.kind() else {
            return Ok(None);
        };

        // looks for embassy_executor::raw::TaskHeader
        if struct_type.name() != Some("TaskHeader")
            || struct_type.namespace().and_then(|n| n.name()) != Some("raw")
            || struct_type
                .namespace()
                .and_then(|n| n.parent())
                .and_then(|n| n.name())
                != Some("embassy_executor")
        {
            return Ok(None);
        }

        let mut state = None;
        for member in struct_type.members() {
            match member.name() {
                Some("state") => {
                    let state_offset = member.bit_offset() / 8;
                    let state_type = match member.bit_size(&file_hash) {
                        Some(8) => StateType::U8,
                        Some(32) => StateType::U32,
                        _ => return Err("Unknown TaskHeader state size".into()),
                    };
                    state = Some((state_offset, state_type))
                }
                _ => {}
            }
        }

        let Some(state) = state else {
            return Err("TaskHeader should have a field `state`".into());
        };

        Ok(Some(Self {
            state_offset: state.0,
            state_type: state.1,
        }))
    }

    pub(crate) fn from_ddbug_data(file_hash: &FileHash<'_>) -> Result<Self> {
        for unit in file_hash.file.units() {
            for unit_type in unit.types() {
                if let Some(result) = Self::from_ddbug_type(unit_type, &file_hash)? {
                    return Ok(result);
                }
            }
        }

        Err("Could not find `TaskHeader` in debug data".into())
    }

    fn is_init(&self, bytes: &[u8]) -> bool {
        let bytes = &bytes[self.state_offset as usize..];

        match self.state_type {
            StateType::U8 => bytes[0] > 0,
            StateType::U32 => u32::from_ne_bytes(bytes[..4].try_into().unwrap()) > 0,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TaskPool {
    pub(crate) path: String,

    // Address where the bytes are stored.
    pub(crate) address: u64,
    // Amount of bytes for the whole pool
    pub(crate) size: u64,
    // The offset of a future from the start of an element in the array
    pub(crate) future_offset: u64,

    // Maximum number of tasks this pool can hold.
    pub(crate) number_of_tasks: usize,

    // The async fn type this pool stores
    pub(crate) async_fn_type: AsyncFnType,
    pub(crate) async_fn_base_type: Type,

    pub(crate) header_layout: HeaderLayout,
}

impl TaskPool {
    pub(crate) fn find_taks_pool<'a>(
        task_name: &str,
        file_hash: &'a FileHash<'a>,
    ) -> Option<&'a ddbug_parser::StructType<'a>> {
        let task_pool_name_prefix = String::from("TaskPool<") + task_name;

        for unit in file_hash.file.units() {
            for unit_type in unit.types() {
                if let ddbug_parser::TypeKind::Struct(struct_type) = unit_type.kind() {
                    if let Some(name) = struct_type.name()
                        && name.starts_with(&task_pool_name_prefix)
                    {
                        return Some(struct_type);
                    }
                }
            }
        }

        return None;
    }

    fn find_future_offset_task_storage(task_storage: &ddbug_parser::Type<'_>) -> Option<u64> {
        match task_storage.kind() {
            TypeKind::Struct(struct_type) => {
                for member in struct_type.members() {
                    if member.name() != Some("future") {
                        continue;
                    }

                    return Some(member.bit_offset() / 8);
                }
                None
            }
            _ => None,
        }
    }

    // TODO: make this work when embassy is compiled with nightly
    pub(crate) fn from_ddbug_var(
        unit_var: &ddbug_parser::Variable<'_>,
        future_types: &HashMap<Type, FutureType>,
        header_layout: &HeaderLayout,
        file_hash: &FileHash<'_>,
    ) -> Result<Option<Self>> {
        if unit_var.name() != Some("POOL") {
            return Ok(None);
        }
        let Some(ty) = unit_var.ty(file_hash) else {
            return Ok(None);
        };
        match ty.kind() {
            ddbug_parser::TypeKind::Struct(struct_type) => {
                let Some(name) = struct_type.name() else {
                    return Ok(None);
                };
                if !name.starts_with("TaskPoolHolder") {
                    return Ok(None);
                }
            }
            _ => return Ok(None),
        }

        let namespace = unit_var
            .namespace()
            .ok_or("TaskPoolHolder needs a namespace")?;

        // The task macro generates a namespace with the name of the function, so the path generated
        // from only the namespaces will actually end in the name of the original task function.
        let path = namespace_to_path(namespace);
        let task_name = namespace_to_path(
            namespace
                .parent()
                .ok_or("TaskPoolHolder's namespace needs a parent")?,
        );
        let task_name = task_name
            + "::__"
            + namespace
                .name()
                .ok_or("TaskPoolHolder's namespace parent needs a name")?
            + "_task";

        let address = unit_var.address().ok_or("TaskPoolHolder needs a address")?;
        let size = unit_var
            .byte_size(file_hash)
            .ok_or("TaskPoolHolder needs a sie")?;

        let task_pool_type = Self::find_taks_pool(&task_name, file_hash).ok_or(format!(
            "Could not find task pool type for task pool: {task_name}"
        ))?;
        let [task_pool_member] = task_pool_type.members() else {
            return Err("TaskPool needs a single member".into());
        };

        let (number_of_tasks, storage_type) = match task_pool_member
            .ty(file_hash)
            .ok_or("TaskPool's member needs a type")?
            .kind()
        {
            ddbug_parser::TypeKind::Array(array_type) => {
                let count = array_type
                    .counts()
                    .next()
                    .flatten()
                    .ok_or("TaskPool inner array needs to have a known size")?;
                let storage_type = array_type
                    .element_type(file_hash)
                    .ok_or("TaskPool need to have a known element type")?;

                (count as usize, storage_type)
            }
            _ => return Err("TaskPool member type needs to be a array".into()),
        };

        let future_offset = Self::find_future_offset_task_storage(&storage_type)
            .ok_or("Could not find future_offset in TaskStorage")?;

        let (async_fn_base_type, async_fn_type) = future_types
            .iter()
            .find(|(ty, _)| match ty {
                Type::Base(name) => name.starts_with(&task_name),
                _ => false,
            })
            .ok_or(format!(
                "Could not find future type for task pool: {}",
                task_name
            ))?;

        let FutureTypeKind::AsyncFn(async_fn_type) = &async_fn_type.kind else {
            return Err("Task pool had a non async fn future type".into());
        };

        Ok(Some(Self {
            path,

            address,
            size,
            future_offset,
            number_of_tasks,

            async_fn_type: async_fn_type.clone(),
            header_layout: header_layout.clone(),
            async_fn_base_type: async_fn_base_type.clone(),
        }))
    }
}

#[derive(Debug)]
pub(crate) enum TaskValue {
    Uninit,
    Init(FutureValue),
}

#[derive(Debug)]
pub(crate) struct TaskPoolValue {
    pub(crate) task_pool: TaskPool,

    pub(crate) task_values: Vec<TaskValue>,
}

impl TaskPoolValue {
    pub(crate) fn new(
        task_pool: &TaskPool,
        bytes: &[u8],
        future_types: &HashMap<Type, FutureType>,
    ) -> Self {
        assert_eq!(bytes.len() as u64, task_pool.size);
        let mut task_values = Vec::new();

        let len_single_task = task_pool.size / task_pool.number_of_tasks as u64;

        for task in 0..task_pool.number_of_tasks {
            let task_offset = len_single_task as usize * task;

            let bytes = &bytes[task_offset..];

            let task_value = if task_pool.header_layout.is_init(bytes) {
                let bytes = &bytes[task_pool.future_offset as usize..];

                TaskValue::Init(FutureValue::async_fn(
                    &task_pool.async_fn_base_type,
                    AsyncFnValue::new(&task_pool.async_fn_type, bytes, future_types),
                ))
            } else {
                TaskValue::Uninit
            };

            task_values.push(task_value)
        }

        Self {
            task_pool: task_pool.clone(),
            task_values,
        }
    }
}
