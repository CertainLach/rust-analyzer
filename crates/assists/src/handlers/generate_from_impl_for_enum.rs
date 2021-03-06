use ast::GenericParamsOwner;
use ide_db::helpers::FamousDefs;
use ide_db::RootDatabase;
use itertools::Itertools;
use stdx::format_to;
use syntax::{
    ast::{self, AstNode, NameOwner},
    SmolStr,
};
use test_utils::mark;

use crate::{AssistContext, AssistId, AssistKind, Assists};

// Assist: generate_from_impl_for_enum
//
// Adds a From impl for an enum variant with one tuple field.
//
// ```
// enum A { $0One(u32) }
// ```
// ->
// ```
// enum A { One(u32) }
//
// impl From<u32> for A {
//     fn from(v: u32) -> Self {
//         Self::One(v)
//     }
// }
// ```
pub(crate) fn generate_from_impl_for_enum(acc: &mut Assists, ctx: &AssistContext) -> Option<()> {
    let variant = ctx.find_node_at_offset::<ast::Variant>()?;
    let variant_name = variant.name()?;
    let enum_name = variant.parent_enum().name()?;
    let enum_type_params = variant.parent_enum().generic_param_list();
    let (field_name, field_type) = match variant.kind() {
        ast::StructKind::Tuple(field_list) => {
            if field_list.fields().count() != 1 {
                return None;
            }
            (None, field_list.fields().next()?.ty()?)
        }
        ast::StructKind::Record(field_list) => {
            if field_list.fields().count() != 1 {
                return None;
            }
            let field = field_list.fields().next()?;
            (Some(field.name()?), field.ty()?)
        }
        ast::StructKind::Unit => return None,
    };

    if existing_from_impl(&ctx.sema, &variant).is_some() {
        mark::hit!(test_add_from_impl_already_exists);
        return None;
    }

    let target = variant.syntax().text_range();
    acc.add(
        AssistId("generate_from_impl_for_enum", AssistKind::Generate),
        "Generate `From` impl for this enum variant",
        target,
        |edit| {
            let start_offset = variant.parent_enum().syntax().text_range().end();
            let mut buf = String::from("\n\nimpl");
            if let Some(type_params) = &enum_type_params {
                format_to!(buf, "{}", type_params.syntax());
            }
            format_to!(buf, " From<{}> for {}", field_type.syntax(), enum_name);
            if let Some(type_params) = enum_type_params {
                let lifetime_params = type_params
                    .lifetime_params()
                    .filter_map(|it| it.lifetime())
                    .map(|it| SmolStr::from(it.text()));
                let type_params = type_params
                    .type_params()
                    .filter_map(|it| it.name())
                    .map(|it| SmolStr::from(it.text()));

                let generic_params = lifetime_params.chain(type_params).format(", ");
                format_to!(buf, "<{}>", generic_params)
            }
            if let Some(name) = field_name {
                format_to!(
                    buf,
                    r#" {{
    fn from({0}: {1}) -> Self {{
        Self::{2} {{ {0} }}
    }}
}}"#,
                    name.text(),
                    field_type.syntax(),
                    variant_name,
                );
            } else {
                format_to!(
                    buf,
                    r#" {{
    fn from(v: {}) -> Self {{
        Self::{}(v)
    }}
}}"#,
                    field_type.syntax(),
                    variant_name,
                );
            }
            edit.insert(start_offset, buf);
        },
    )
}

fn existing_from_impl(
    sema: &'_ hir::Semantics<'_, RootDatabase>,
    variant: &ast::Variant,
) -> Option<()> {
    let variant = sema.to_def(variant)?;
    let enum_ = variant.parent_enum(sema.db);
    let krate = enum_.module(sema.db).krate();

    let from_trait = FamousDefs(sema, Some(krate)).core_convert_From()?;

    let enum_type = enum_.ty(sema.db);

    let wrapped_type = variant.fields(sema.db).get(0)?.signature_ty(sema.db);

    if enum_type.impls_trait(sema.db, from_trait, &[wrapped_type]) {
        Some(())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use test_utils::mark;

    use crate::tests::{check_assist, check_assist_not_applicable};

    use super::*;

    #[test]
    fn test_generate_from_impl_for_enum() {
        check_assist(
            generate_from_impl_for_enum,
            "enum A { $0One(u32) }",
            r#"enum A { One(u32) }

impl From<u32> for A {
    fn from(v: u32) -> Self {
        Self::One(v)
    }
}"#,
        );
    }

    #[test]
    fn test_generate_from_impl_for_enum_complicated_path() {
        check_assist(
            generate_from_impl_for_enum,
            r#"enum A { $0One(foo::bar::baz::Boo) }"#,
            r#"enum A { One(foo::bar::baz::Boo) }

impl From<foo::bar::baz::Boo> for A {
    fn from(v: foo::bar::baz::Boo) -> Self {
        Self::One(v)
    }
}"#,
        );
    }

    fn check_not_applicable(ra_fixture: &str) {
        let fixture =
            format!("//- /main.rs crate:main deps:core\n{}\n{}", ra_fixture, FamousDefs::FIXTURE);
        check_assist_not_applicable(generate_from_impl_for_enum, &fixture)
    }

    #[test]
    fn test_add_from_impl_no_element() {
        check_not_applicable("enum A { $0One }");
    }

    #[test]
    fn test_add_from_impl_more_than_one_element_in_tuple() {
        check_not_applicable("enum A { $0One(u32, String) }");
    }

    #[test]
    fn test_add_from_impl_struct_variant() {
        check_assist(
            generate_from_impl_for_enum,
            "enum A { $0One { x: u32 } }",
            r#"enum A { One { x: u32 } }

impl From<u32> for A {
    fn from(x: u32) -> Self {
        Self::One { x }
    }
}"#,
        );
    }

    #[test]
    fn test_add_from_impl_already_exists() {
        mark::check!(test_add_from_impl_already_exists);
        check_not_applicable(
            r#"
enum A { $0One(u32), }

impl From<u32> for A {
    fn from(v: u32) -> Self {
        Self::One(v)
    }
}
"#,
        );
    }

    #[test]
    fn test_add_from_impl_different_variant_impl_exists() {
        check_assist(
            generate_from_impl_for_enum,
            r#"enum A { $0One(u32), Two(String), }

impl From<String> for A {
    fn from(v: String) -> Self {
        A::Two(v)
    }
}

pub trait From<T> {
    fn from(T) -> Self;
}"#,
            r#"enum A { One(u32), Two(String), }

impl From<u32> for A {
    fn from(v: u32) -> Self {
        Self::One(v)
    }
}

impl From<String> for A {
    fn from(v: String) -> Self {
        A::Two(v)
    }
}

pub trait From<T> {
    fn from(T) -> Self;
}"#,
        );
    }

    #[test]
    fn test_add_from_impl_static_str() {
        check_assist(
            generate_from_impl_for_enum,
            "enum A { $0One(&'static str) }",
            r#"enum A { One(&'static str) }

impl From<&'static str> for A {
    fn from(v: &'static str) -> Self {
        Self::One(v)
    }
}"#,
        );
    }

    #[test]
    fn test_add_from_impl_generic_enum() {
        check_assist(
            generate_from_impl_for_enum,
            "enum Generic<T, U: Clone> { $0One(T), Two(U) }",
            r#"enum Generic<T, U: Clone> { One(T), Two(U) }

impl<T, U: Clone> From<T> for Generic<T, U> {
    fn from(v: T) -> Self {
        Self::One(v)
    }
}"#,
        );
    }

    #[test]
    fn test_add_from_impl_with_lifetime() {
        check_assist(
            generate_from_impl_for_enum,
            "enum Generic<'a> { $0One(&'a i32) }",
            r#"enum Generic<'a> { One(&'a i32) }

impl<'a> From<&'a i32> for Generic<'a> {
    fn from(v: &'a i32) -> Self {
        Self::One(v)
    }
}"#,
        );
    }
}
