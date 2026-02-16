use argus_difflens::parser::parse_unified_diff;
use std::path::PathBuf;

#[test]
fn parse_patch_without_git_header() {
    let diff = "\
--- /dev/null
+++ b/examples/bad_code.rs
@@ -0,0 +1,13 @@
+fn main() {
+    println!(\"hello\");
+}
";
    let files = parse_unified_diff(diff).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].new_path, PathBuf::from("examples/bad_code.rs"));
    assert!(files[0].is_new_file || files[0].hunks[0].change_type == argus_core::ChangeType::Add);
}
