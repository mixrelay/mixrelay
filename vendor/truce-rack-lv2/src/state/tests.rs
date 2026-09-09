use super::*;

fn encoded(properties: Vec<Property>) -> Vec<u8> {
    serde_json::to_vec(&Snapshot {
        version: 1,
        plugin_uri: "urn:tonenari:state-fixture".into(),
        byte_order: byte_order().into(),
        native_state: true,
        ports: BTreeMap::new(),
        properties,
    })
    .unwrap()
}

#[test]
fn opaque_binary_property_is_owned_and_urid_values_remap_between_instances() {
    let first = UridMap::new();
    let key = map_uri(&first, "urn:fixture:opaque").unwrap();
    let value_type = map_uri(&first, "urn:fixture:binary").unwrap();
    let mut context = StoreContext {
        map: &first,
        properties: Vec::new(),
        bytes: 0,
        failure: None,
    };
    let mut bytes = [0_u8, 255, 13, 0];
    let status = unsafe {
        store(
            (&mut context as *mut StoreContext<'_>).cast(),
            key,
            bytes.as_ptr().cast(),
            bytes.len(),
            value_type,
            PORTABLE_POD,
        )
    };
    assert_eq!(status, 0);
    bytes.fill(99);
    let urid_key = map_uri(&first, "urn:fixture:selected").unwrap();
    let urid_type = map_uri(&first, &format!("{ATOM}URID")).unwrap();
    let urid = map_uri(&first, "urn:fixture:value").unwrap();
    assert_eq!(
        unsafe {
            store(
                (&mut context as *mut StoreContext<'_>).cast(),
                urid_key,
                (&urid as *const u32).cast(),
                4,
                urid_type,
                PORTABLE_POD,
            )
        },
        0
    );
    let encoded = encoded(context.properties);
    let second = UridMap::new();
    for id in 0..10 {
        map_uri(&second, &format!("urn:other:{id}")).unwrap();
    }
    let (_, restored) = decode(&encoded, "urn:tonenari:state-fixture", &second).unwrap();
    assert_eq!(restored[0].bytes, [0, 255, 13, 0]);
    assert_ne!(restored[0].key, key);
    assert_eq!(
        u32::from_ne_bytes(restored[1].bytes.clone().try_into().unwrap()),
        map_uri(&second, "urn:fixture:value").unwrap()
    );
    let mut size = 0;
    let mut value_type = 0;
    let mut flags = 0;
    let raw = unsafe {
        retrieve(
            (&restored as *const Vec<RestoreProperty>).cast_mut().cast(),
            restored[0].key,
            &mut size,
            &mut value_type,
            &mut flags,
        )
    };
    assert_eq!(
        unsafe { std::slice::from_raw_parts(raw.cast::<u8>(), size) },
        [0, 255, 13, 0]
    );
    assert_eq!(flags, PORTABLE_POD);
    assert_eq!(value_type, restored[0].value_type);
}

#[test]
fn invalid_nonportable_oversized_and_asset_properties_are_rejected() {
    let map = UridMap::new();
    let key = map_uri(&map, "urn:fixture:key").unwrap();
    let blob_type = map_uri(&map, "urn:fixture:blob").unwrap();
    let path_type = map_uri(&map, &format!("{ATOM}Path")).unwrap();
    let value = [1_u8; 4];
    for (value_type, flags, size) in [
        (blob_type, 1, 4),
        (blob_type, 3, PROPERTY_LIMIT + 1),
        (path_type, 3, 4),
        (blob_type, 3, 0),
    ] {
        let mut context = StoreContext {
            map: &map,
            properties: Vec::new(),
            bytes: 0,
            failure: None,
        };
        assert_ne!(
            unsafe {
                store(
                    (&mut context as *mut StoreContext<'_>).cast(),
                    key,
                    value.as_ptr().cast(),
                    size,
                    value_type,
                    flags,
                )
            },
            0
        );
        assert!(context.failure.is_some());
        assert!(context.properties.is_empty());
    }
    let malformed = encoded(vec![Property {
        key: "urn:fixture:key".into(),
        value_type: format!("{ATOM}Path"),
        value: PropertyValue::Bytes(b"/file\0".to_vec()),
    }]);
    assert!(decode(&malformed, "urn:tonenari:state-fixture", &map).is_err());
    assert!(decode(&encoded(Vec::new()), "urn:different-plugin", &map).is_err());
    assert!(decode(b"broken", "urn:tonenari:state-fixture", &map).is_err());
}

/// Run only with the repository-owned fixture bundle compiled and `LV2_PATH` pointing to it.
/// This is an actual Lilv-loaded shared object, not a mock of the vendor state interface.
#[test]
#[ignore = "requires compiled fixtures/state.lv2 and isolated LV2_PATH"]
fn native_state_fixture_restores_opaque_bytes_and_ports_and_propagates_vendor_failures() {
    use truce_rack_core::{plugin::PluginCore, scanner::PluginScanner};
    let scanner = super::super::Lv2Scanner::new();
    let info = scanner
        .scan()
        .unwrap()
        .into_iter()
        .find(|info| info.unique_id == "urn:tonenari:state-fixture")
        .expect("compile the repository-owned fixture and set isolated LV2_PATH");
    let mut plugin = scanner.load(&info).unwrap();
    plugin
        .activate(plugin.layouts[0].clone(), 48_000.0, 128)
        .unwrap();
    let original = plugin.save_state().unwrap();
    let mut snapshot: Snapshot = serde_json::from_slice(&original).unwrap();
    assert_eq!(
        snapshot.properties.len(),
        1,
        "opaque native state, not just control ports"
    );
    assert_eq!(snapshot.ports.len(), 1);
    snapshot.properties[0].value = PropertyValue::Bytes(vec![9, 8, 7, 0, 255]);
    snapshot.ports.insert("gain".into(), 0.25);
    plugin
        .load_state(&serde_json::to_vec(&snapshot).unwrap())
        .unwrap();
    let recalled: Snapshot = serde_json::from_slice(&plugin.save_state().unwrap()).unwrap();
    assert!((recalled.ports["gain"] - 0.25).abs() < f32::EPSILON);
    assert!(
        matches!(&recalled.properties[0].value, PropertyValue::Bytes(bytes) if bytes == &[9,8,7,0,255])
    );
    // The fixture rejects a leading 254 on restore, and a leading 253 on subsequent save.
    snapshot.properties[0].value = PropertyValue::Bytes(vec![254, 0, 0, 0, 0]);
    assert!(
        plugin
            .load_state(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap_err()
            .to_string()
            .contains("restore failed with status 1")
    );
    snapshot.properties[0].value = PropertyValue::Bytes(vec![253, 0, 0, 0, 0]);
    plugin
        .load_state(&serde_json::to_vec(&snapshot).unwrap())
        .unwrap();
    assert!(
        plugin
            .save_state()
            .unwrap_err()
            .to_string()
            .contains("save failed with status 1")
    );
}
