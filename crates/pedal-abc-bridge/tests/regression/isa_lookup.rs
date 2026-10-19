//! ISA 查找冒烟测试
//!
//! @author ljlVink

#[test]
fn lookup_known_opcodes() {
    assert!(
        pedal_abc_bridge::isa::lookup(None, 0x91).is_some(),
        "stobjbyname 0x91"
    );
    assert!(
        pedal_abc_bridge::isa::lookup(None, 0x93).is_some(),
        "ldthisbyname 0x93"
    );
    assert!(pedal_abc_bridge::isa::lookup(None, 0x00).is_some());
    let i = pedal_abc_bridge::isa::lookup(None, 0x91).unwrap();
    assert_eq!(i.mnemonic, "stobjbyname");
    let data = [0x91, 0x00, 0x00, 0x0c, 0x7d, 0x00];
    let ins = pedal_abc_bridge::disasm::decode(&data).unwrap();
    assert_eq!(ins.len(), 1);
    assert_eq!(ins[0].size, 6);
}
