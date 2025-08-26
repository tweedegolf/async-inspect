use ddbug_parser::{FileHash, Result, TypeKind};

use super::{
    async_fn::{AsyncFnType, AsyncFnValue},
    ty::Type,
};

#[derive(Debug, Clone)]
pub(crate) struct SelectFuture {
    pub(crate) awaitees: Box<[(u64, Type)]>,
}

impl SelectFuture {
    fn from_ddbug_select_array(
        ddbug_type: &ddbug_parser::StructType<'_>,
        file_hash: &FileHash<'_>,
    ) -> Result<Self> {
        let [inner] = ddbug_type.members() else {
            return Err("Expected SelectArray to have a single field".into());
        };

        let ty = inner.ty(file_hash);
        let array_type = match ty.as_ref().map(|ty| ty.kind()) {
            Some(TypeKind::Array(array_type)) => array_type,
            other => {
                return Err(format!(
                    "Expected SelectArray's inner field to have a array type, not: {other:?}"
                )
                .into());
            }
        };

        let ty = array_type.element_type(file_hash);
        let ty = Type::from_maybe_ddbug_type(ty, file_hash);

        let count = array_type
            .counts()
            .next()
            .flatten()
            .ok_or("Could not determain the count of the SelectArray")?;
        let size = array_type
            .byte_size(file_hash)
            .ok_or("Could not determain the size of the SelectArray")?;
        let size_of_element = size / count;

        let awaitees = (0..count)
            .map(|i| (size_of_element * i, ty.clone()))
            .collect();

        Ok(Self { awaitees })
    }

    fn from_ddbug_select_fixed_size(
        ddbug_type: &ddbug_parser::StructType<'_>,
        file_hash: &FileHash<'_>,
    ) -> Result<Self> {
        let awaitees = ddbug_type
            .members()
            .into_iter()
            .map(|member| {
                let offset = member.bit_offset() / 8;
                let ty = Type::from_maybe_ddbug_type(member.ty(file_hash), file_hash);
                (offset, ty)
            })
            .collect();

        Ok(Self { awaitees })
    }

    fn from_ddbug_type(
        ddbug_type: &ddbug_parser::Type<'_>,
        file_hash: &FileHash<'_>,
    ) -> Result<Option<Self>> {
        let TypeKind::Struct(struct_type) = ddbug_type.kind() else {
            return Ok(None);
        };

        // Looks for paths starting embassy_futures::select
        if struct_type.namespace().and_then(|n| n.name()) != Some("select")
            || struct_type
                .namespace()
                .and_then(|n| n.parent())
                .and_then(|n| n.name())
                != Some("embassy_futures")
        {
            return Ok(None);
        }
        let Some(name) = struct_type.name() else {
            return Ok(None);
        };

        if name.starts_with("SelectArray") {
            let future = Self::from_ddbug_select_array(struct_type, file_hash)?;
            return Ok(Some(future));
        }

        const FIXED_SIZE_NAMES: &[&str; 3] = &["Select<", "Select3<", "Select4<"];
        if FIXED_SIZE_NAMES
            .iter()
            .any(|fixed_size_name| name.starts_with(fixed_size_name))
        {
            let future = Self::from_ddbug_select_fixed_size(struct_type, file_hash)?;
            return Ok(Some(future));
        }

        Ok(None)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum FutureTypeKind {
    AsyncFn(AsyncFnType),
    Select(SelectFuture),
}

#[derive(Debug, Clone)]
pub(crate) struct FutureType {
    pub(crate) ty: Type,
    pub(crate) kind: FutureTypeKind,
}

impl FutureType {
    pub(crate) fn from_ddbug_type(
        ddbug_type: &ddbug_parser::Type<'_>,
        file_hash: &FileHash,
    ) -> Result<Option<Self>> {
        let ty = Type::from_ddbug_type(ddbug_type, file_hash);

        if let Some(async_fn_type) = AsyncFnType::from_ddbug_type(ddbug_type, file_hash)? {
            return Ok(Some(Self {
                ty,
                kind: FutureTypeKind::AsyncFn(async_fn_type),
            }));
        }

        if let Some(join_future_type) = SelectFuture::from_ddbug_type(ddbug_type, file_hash)? {
            return Ok(Some(Self {
                ty,
                kind: FutureTypeKind::Select(join_future_type),
            }));
        }

        Ok(None)
    }
}

#[derive(Debug)]
pub(crate) struct SelectValue {
    pub(crate) ty: SelectFuture,
    pub(crate) awaitees: Box<[FutureValue]>,
}

impl SelectValue {
    fn new(
        select_type: &SelectFuture,
        bytes: &[u8],
        future_types: &std::collections::HashMap<Type, FutureType>,
    ) -> Self {
        let awaitees = select_type
            .awaitees
            .iter()
            .map(|(offset, ty)| {
                let bytes = &bytes[*offset as usize..];
                FutureValue::new(ty, bytes, future_types)
            })
            .collect();

        Self {
            ty: select_type.clone(),
            awaitees,
        }
    }
}

#[derive(Debug)]
pub(crate) enum FutureValueKind {
    AsyncFn(AsyncFnValue),
    SelectValue(SelectValue),
    Unknown(Vec<u8>),
}

#[derive(Debug)]
pub(crate) struct FutureValue {
    pub(crate) ty: Type,
    pub(crate) kind: FutureValueKind,
}

impl FutureValue {
    pub(crate) fn new(
        ty: &Type,
        bytes: &[u8],
        future_types: &std::collections::HashMap<Type, FutureType>,
    ) -> Self {
        let future_type = future_types.get(ty);

        let kind = match future_type.map(|f| &f.kind) {
            Some(FutureTypeKind::AsyncFn(async_fn_type)) => {
                FutureValueKind::AsyncFn(AsyncFnValue::new(async_fn_type, bytes, future_types))
            }
            Some(FutureTypeKind::Select(select_type)) => {
                FutureValueKind::SelectValue(SelectValue::new(select_type, bytes, future_types))
            }
            None => FutureValueKind::Unknown(bytes.to_vec()),
        };

        Self {
            ty: ty.clone(),
            kind,
        }
    }

    pub(crate) fn async_fn(ty: &Type, async_fn_value: AsyncFnValue) -> FutureValue {
        Self {
            ty: ty.clone(),
            kind: FutureValueKind::AsyncFn(async_fn_value),
        }
    }
}
