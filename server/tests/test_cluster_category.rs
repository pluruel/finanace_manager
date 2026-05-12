mod common;

#[test]
fn scope_category_is_valid() {
    use finance_manager::api::clusters::Scope;
    assert!(Scope::parse("category").is_some());
    assert!(Scope::parse("product").is_some());
    assert!(Scope::parse("merchant").is_some());
    assert!(Scope::parse("invalid").is_none());
}

#[test]
fn scope_category_entity_table() {
    use finance_manager::api::clusters::Scope;
    assert_eq!(Scope::Category.entity_table(), "categories");
    assert_eq!(Scope::Category.fk_column(), "category_id");
}
