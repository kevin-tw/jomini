#[test]
fn reject_bin_obj_in_hidden_obj() {
    let data = include_bytes!("./fixtures/nested-hidden-obj.bin");
    assert!(jomini::BinaryParser::from_eu4(&data[..]).is_err());
}

#[test]
fn reject_txt_obj_in_hidden_obj() {
    let data = include_bytes!("./fixtures/nested-hidden-obj.txt");
    assert!(jomini::TextParser::from_windows1252(&data[..]).is_err());
}
