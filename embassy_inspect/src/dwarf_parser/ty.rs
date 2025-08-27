use ddbug_parser::{FileHash, TypeKind};

use super::from_namespace_and_name;

/// This type mostly only exsit to work around gdb bugs.
///
/// GDB does not seem to recognize types of the form `[<type>; 123]` or `*u8`, but these can
/// be manualy created. So this type used to store the "shape" of some types to allow for the
/// reconstruction on the GDB side.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    #[default]
    Unknown,
    Void,
    /// Fixed size array: `[inner; count]`
    Array {
        inner: Box<Type>,
        count: u64,
    },
    Pointer(Box<Type>),
    Refrence(Box<Type>),
    /// Any type that has a clear name
    Base(String),
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Unknown => write!(f, "<unknown>"),
            Type::Void => write!(f, "void"),
            Type::Array { inner, count } => write!(f, "[{inner}; {count}]"),
            Type::Pointer(inner) => write!(f, "*{inner}"),
            Type::Refrence(inner) => write!(f, "&{inner}"),
            Type::Base(name) => write!(f, "{name}"),
        }
    }
}

impl Type {
    fn from_namespace_and_name(
        namespace: Option<&ddbug_parser::Namespace<'_>>,
        name: Option<&str>,
    ) -> Self {
        Self::Base(from_namespace_and_name(namespace, name))
    }

    pub(crate) fn from_ddbug_type(ty: &ddbug_parser::Type, file_hash: &FileHash) -> Self {
        match ty.kind() {
            TypeKind::Void => Self::Void,
            TypeKind::Base(base_type) => {
                Self::Base(base_type.name().unwrap_or("<unknown>").to_owned())
            }
            TypeKind::Def(type_def) => {
                Self::from_namespace_and_name(type_def.namespace(), type_def.name())
            }
            TypeKind::Struct(struct_type) => {
                Self::from_namespace_and_name(struct_type.namespace(), struct_type.name())
            }
            TypeKind::Union(union_type) => {
                Self::from_namespace_and_name(union_type.namespace(), union_type.name())
            }
            TypeKind::Enumeration(enumeration_type) => {
                Self::from_namespace_and_name(enumeration_type.namespace(), enumeration_type.name())
            }
            TypeKind::Array(array_type) => {
                let inner =
                    Self::from_maybe_ddbug_type(array_type.element_type(file_hash), file_hash);

                match array_type.counts().collect::<Vec<_>>().as_slice() {
                    [Some(count)] => Self::Array {
                        inner: Box::new(inner),
                        count: *count,
                    },
                    [] | [None] => Self::Unknown,
                    _ => Self::Unknown,
                }
            }
            TypeKind::Function(function_type) => {
                // Building to a base type to a string as GDB does not have a way to build function
                // types anyway.
                // TODO: The syntax here techicaly returns a ptr, so a wraping [`Self::Pointer`]
                // sould be removed.
                let mut name = String::from("fn(");
                let parameters = function_type
                    .parameters()
                    .iter()
                    .map(|par| {
                        Self::from_maybe_ddbug_type(par.ty(file_hash), file_hash).to_string()
                    })
                    .collect::<Vec<_>>();
                name.push_str(&parameters.join(","));
                name.push_str(")");

                if let Some(ret) = function_type.return_type(file_hash) {
                    name.push_str(" -> ");
                    name.push_str(&Self::from_ddbug_type(&ret, file_hash).to_string());
                }

                Self::Base(name)
            }
            TypeKind::Unspecified(unspecified_type) => {
                Self::from_namespace_and_name(unspecified_type.namespace(), unspecified_type.name())
            }
            TypeKind::PointerToMember(pointer_to_member_type) => {
                let inner = Self::from_maybe_ddbug_type(
                    pointer_to_member_type.member_type(file_hash),
                    file_hash,
                );
                Self::Pointer(Box::new(inner))
            }
            TypeKind::Modifier(type_modifier) => {
                let inner = Self::from_maybe_ddbug_type(type_modifier.ty(file_hash), file_hash);

                match type_modifier.kind() {
                    ddbug_parser::TypeModifierKind::Pointer => Self::Pointer(Box::new(inner)),
                    ddbug_parser::TypeModifierKind::Reference => Self::Refrence(Box::new(inner)),
                    ddbug_parser::TypeModifierKind::Const
                    | ddbug_parser::TypeModifierKind::Packed
                    | ddbug_parser::TypeModifierKind::Volatile
                    | ddbug_parser::TypeModifierKind::Restrict
                    | ddbug_parser::TypeModifierKind::Shared
                    | ddbug_parser::TypeModifierKind::RvalueReference
                    | ddbug_parser::TypeModifierKind::Atomic
                    | ddbug_parser::TypeModifierKind::Other => inner,
                }
            }
            TypeKind::Subrange(subrange_type) => subrange_type
                .ty(file_hash)
                .map(|inner| Self::from_ddbug_type(&inner, file_hash))
                .unwrap_or_else(|| Self::Unknown),
        }
    }

    /// Helper that returns [`Self::Unknown`] if `ty` is `None` and forwards the type to
    /// [`Self::from_ddbug_type`] otherwise.
    pub(crate) fn from_maybe_ddbug_type(
        ty: Option<std::borrow::Cow<'_, ddbug_parser::Type>>,
        file_hash: &FileHash,
    ) -> Self {
        ty.map(|ty| Self::from_ddbug_type(&ty, file_hash))
            .unwrap_or_default()
    }
}
