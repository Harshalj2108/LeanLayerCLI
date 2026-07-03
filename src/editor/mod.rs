pub mod buffer;
pub mod errors;
pub mod syntax;

#[cfg(test)]
mod tests {
    #[test]
    fn editor_modules_are_exported() {
        let _mode = crate::editor::buffer::EditorMode::Normal;
    }
}
