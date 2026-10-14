//! Stripping TypeScript types, so a hook file can be `.pb.ts`.
//!
//! Only types are removed — annotations, interfaces, `as` casts, generics. The
//! output is the same program with the type syntax gone, which is exactly what
//! QuickJS needs and avoids a bundler or a build step.

use std::path::Path;

use lazypock_core::error::{Error, Result};
use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{TransformOptions, Transformer};

/// Strip the types from a TypeScript hook file.
pub fn strip_types(source: &str, filename: &str) -> Result<String> {
    let source_type = SourceType::from_path(filename)
        .map_err(|err| Error::invalid(format!("cannot read the language of {filename}: {err}")))?;

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if let Some(error) = parsed.diagnostics.first() {
        // A compile error is reported with its position: this is the message the
        // hook author has to act on, and they cannot debug a stack trace.
        return Err(Error::invalid(format!("{filename}: {error}")));
    }

    let mut program = parsed.program;
    let scoping = SemanticBuilder::new()
        .build(&program)
        .semantic
        .into_scoping();

    Transformer::new(
        &allocator,
        Path::new(filename),
        &TransformOptions::default(),
    )
    .build_with_scoping(scoping, &mut program);

    Ok(Codegen::new().build(&program).code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_annotations_are_removed() {
        let stripped = strip_types(
            "const greet = (name: string): string => `hi ${name}`;",
            "hook.pb.ts",
        )
        .expect("strip");
        assert!(!stripped.contains("string"), "{stripped}");
        assert!(stripped.contains("name"), "{stripped}");
    }
}
