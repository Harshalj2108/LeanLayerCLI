pub mod tree;

#[cfg(test)]
mod tests {
    #[test]
    fn explorer_module_is_exported() {
        let _ = std::mem::size_of::<crate::explorer::tree::ExplorerState>();
    }
}
