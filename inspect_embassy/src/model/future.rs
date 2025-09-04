//! Models for the memory layout of join and select futures.

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
pub(crate) struct JoinAwaiteeTypeVariant {
    pub(crate) discriminant: u64,
    pub(crate) offset: u64,
    pub(crate) size: u64,
    pub(crate) ty: Type,
}

#[derive(Debug, Clone)]
pub(crate) struct JoinAwaiteeType {
    pub(crate) discriminant_offset: u64,
    pub(crate) discriminant_size: u64,

    pub(crate) future_variant: JoinAwaiteeTypeVariant,
    pub(crate) done_variant: JoinAwaiteeTypeVariant,
}

impl JoinAwaiteeType {
    fn from_ddbug_type(ty: &ddbug_parser::Type, file_hash: &FileHash) -> Option<Self> {
        let TypeKind::Struct(ty) = ty.kind() else {
            return None;
        };
        let [variant_part] = ty.variant_parts() else {
            return None;
        };

        let discriminant = variant_part.discriminant(ty.members())?;
        let discriminant_offset = discriminant.bit_offset() / 8;
        let discriminant_size = discriminant.bit_size(file_hash)? / 8;

        let mut future_variant = None;
        let mut done_variant = None;
        for variant in variant_part.variants() {
            let name = variant.name()?;
            if name == "Gone" {
                continue;
            }

            let [member] = variant.members() else {
                return None;
            };

            let variant = JoinAwaiteeTypeVariant {
                discriminant: variant.discriminant_value()?,
                offset: member.bit_offset() / 8,
                size: member.bit_size(file_hash)? / 8,
                ty: Type::from_maybe_ddbug_type(member.ty(file_hash), file_hash),
            };
            match name {
                "Future" => future_variant = Some(variant),
                "Done" => done_variant = Some(variant),
                _ => return None,
            }
        }

        Some(Self {
            discriminant_offset,
            discriminant_size,

            future_variant: future_variant?,
            done_variant: done_variant?,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct JoinFuture {
    pub(crate) awaitees: Box<[(u64, JoinAwaiteeType)]>,
}

impl JoinFuture {
    fn from_ddbug_select_array(
        ddbug_type: &ddbug_parser::StructType<'_>,
        file_hash: &FileHash<'_>,
    ) -> Result<Self> {
        let [inner] = ddbug_type.members() else {
            return Err("Expected JoinArray to have a single field".into());
        };

        let ty = inner.ty(file_hash);
        let array_type = match ty.as_ref().map(|ty| ty.kind()) {
            Some(TypeKind::Array(array_type)) => array_type,
            other => {
                return Err(format!(
                    "Expected JoinArray's inner field to have a array type, not: {other:?}"
                )
                .into());
            }
        };

        let ty = array_type
            .element_type(file_hash)
            .ok_or("Expected JoinArray to have a known inner type")?;
        let ty = JoinAwaiteeType::from_ddbug_type(&ty, file_hash)
            .ok_or("JoinArray has a unexpected MaybeDone enum layout")?;

        let count = array_type
            .counts()
            .next()
            .flatten()
            .ok_or("Could not determain the count of the JoinArray")?;
        let size = array_type
            .byte_size(file_hash)
            .ok_or("Could not determain the size of the JoinArray")?;
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
            .map(|member| -> Result<_> {
                let offset = member.bit_offset() / 8;
                let ty = member
                    .ty(file_hash)
                    .ok_or("Expected JoinArray to have a known inner type")?;
                let ty = JoinAwaiteeType::from_ddbug_type(&ty, file_hash)
                    .ok_or("Expected JoinArray has a unknown MaybeDone enum layout")?;
                Ok((offset, ty))
            })
            .collect::<Result<_>>()?;

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
        if struct_type.namespace().and_then(|n| n.name()) != Some("join")
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

        if name.starts_with("JoinArray") {
            let future = Self::from_ddbug_select_array(struct_type, file_hash)?;
            return Ok(Some(future));
        }

        const FIXED_SIZE_NAMES: &[&str; 3] = &["Join<", "Join3<", "Join4<"];
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
    Join(JoinFuture),
}

#[derive(Debug, Clone)]
pub(crate) struct FutureType {
    pub(crate) kind: FutureTypeKind,
}

impl FutureType {
    pub(crate) fn from_ddbug_type(
        ddbug_type: &ddbug_parser::Type<'_>,
        file_hash: &FileHash,
    ) -> Result<Option<Self>> {
        if let Some(async_fn_type) = AsyncFnType::from_ddbug_type(ddbug_type, file_hash)? {
            return Ok(Some(Self {
                kind: FutureTypeKind::AsyncFn(async_fn_type),
            }));
        }

        if let Some(select_future_type) = SelectFuture::from_ddbug_type(ddbug_type, file_hash)? {
            return Ok(Some(Self {
                kind: FutureTypeKind::Select(select_future_type),
            }));
        }

        if let Some(join_future_type) = JoinFuture::from_ddbug_type(ddbug_type, file_hash)? {
            return Ok(Some(Self {
                kind: FutureTypeKind::Join(join_future_type),
            }));
        }

        Ok(None)
    }
}

#[derive(Debug)]
pub(crate) struct SelectValue {
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

        Self { awaitees }
    }
}

#[derive(Debug)]
pub(crate) struct JoinValue {
    pub(crate) awaitees: Box<[FutureValue]>,
}

impl JoinValue {
    fn new(
        select_type: &JoinFuture,
        bytes: &[u8],
        future_types: &std::collections::HashMap<Type, FutureType>,
    ) -> Self {
        let awaitees = select_type
            .awaitees
            .iter()
            .map(|(offset, ty)| {
                let bytes = &bytes[*offset as usize..];

                let disc_bytes = &bytes[ty.discriminant_offset as usize..];
                let discriminant = match ty.discriminant_size {
                    1 => u8::from_le_bytes(disc_bytes[..1].try_into().unwrap()) as u64,
                    2 => u16::from_le_bytes(disc_bytes[..2].try_into().unwrap()) as u64,
                    4 => u32::from_le_bytes(disc_bytes[..4].try_into().unwrap()) as u64,
                    8 => u64::from_le_bytes(disc_bytes[..8].try_into().unwrap()),
                    _ => unreachable!(),
                };

                if discriminant == ty.future_variant.discriminant {
                    let bytes = &bytes[ty.future_variant.offset as usize..]
                        [..ty.future_variant.size as usize];
                    FutureValue::new(&ty.future_variant.ty, bytes, future_types)
                } else if discriminant == ty.done_variant.discriminant {
                    let bytes =
                        &bytes[ty.done_variant.offset as usize..][..ty.done_variant.size as usize];

                    FutureValue {
                        ty: ty.done_variant.ty.clone(),
                        kind: FutureValueKind::Unknown(bytes.to_vec()),
                    }
                } else {
                    // The value has been taken by calling `take_output`
                    // TODO: return something more usefull
                    FutureValue {
                        ty: Type::Void,
                        kind: FutureValueKind::Unknown(Vec::new()),
                    }
                }
            })
            .collect();

        Self { awaitees }
    }
}

#[derive(Debug)]
pub(crate) enum FutureValueKind {
    AsyncFn(AsyncFnValue),
    SelectValue(SelectValue),
    JoinValue(JoinValue),
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
            Some(FutureTypeKind::Join(join_type)) => {
                FutureValueKind::JoinValue(JoinValue::new(join_type, bytes, future_types))
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
