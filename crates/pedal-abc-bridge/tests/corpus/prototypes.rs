//! 系统框架真实静态方法原型回归。
//! @author ljlVink

use pedal_abc_bridge::{
    file::{AbcFile, BasicType, Type},
    reader::Reader,
};

#[test]
#[ignore = "requires optional test_examples corpus; see tests/README.md"]
fn camera_static_corpus_contains_real_typed_prototypes() {
    let path = super::corpus_path("system_framework/camera_framework_taihe_abc.abc");
    let file = AbcFile::parse(std::fs::read(path).unwrap()).unwrap();
    let mut methods = 0;
    let mut with_receiver = 0;
    let mut references = 0;
    for class in &file.classes {
        for method in &class.methods {
            let proto = file
                .read_proto(method.proto_off)
                .unwrap_or_else(|error| panic!("{}::{}: {error}", class.name, method.name));
            methods += 1;
            references += proto.ref_types.len();
            assert_eq!(method.class_off, class.offset);
            if let Some(code) = &method.code {
                let explicit = proto.params.len() as u32;
                let mut reader = Reader::new(&file.data);
                reader.seek(method.offset as usize + 8).unwrap();
                let is_static = reader.uleb().unwrap() & 0x8 != 0;
                assert_eq!(
                    code.num_args,
                    explicit + u32::from(!is_static),
                    "{}::{} code args {} proto args {}",
                    class.name,
                    method.name,
                    code.num_args,
                    explicit
                );
                with_receiver += usize::from(code.num_args == explicit + 1);
            }
        }
    }
    assert!(methods > 7000);
    assert!(with_receiver > 1000);
    assert!(references > 1000);

    let zoom = file
        .classes
        .iter()
        .find(|class| class.name == "L@ohos/multimedia/camera/camera/_taihe_Zoom_inner;")
        .unwrap();
    let setter = zoom
        .methods
        .iter()
        .find(|method| method.name == "setZoomRatio")
        .unwrap();
    let proto = file.read_proto(setter.proto_off).unwrap();
    assert_eq!(proto.ret, Type::Basic(BasicType::Void));
    assert_eq!(proto.params, [Type::Basic(BasicType::F64)]);
    assert_eq!(setter.code.as_ref().unwrap().num_args, 2);

    let camera = file
        .classes
        .iter()
        .find(|class| class.name == "L@ohos/multimedia/camera/camera;")
        .unwrap();
    let manager = camera
        .methods
        .iter()
        .find(|method| method.name == "getCameraManager")
        .unwrap();
    let proto = file.read_proto(manager.proto_off).unwrap();
    assert_eq!(proto.ref_types.len(), 2);
    assert_eq!(
        file.class_name(proto.ref_types[0]).unwrap(),
        "L@ohos/multimedia/camera/camera/CameraManager;"
    );
    assert_eq!(
        file.class_name(proto.ref_types[1]).unwrap(),
        "Lapplication/BaseContext/BaseContext;"
    );
    assert_eq!(manager.code.as_ref().unwrap().num_args, 1);
}
