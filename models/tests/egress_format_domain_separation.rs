use models::rust::channel_image::{
    ChannelImageError, ChannelImageV2, ValidatedChannelImage, CHANNEL_IMAGE_V2_DOMAIN,
};
use models::rust::epathmap_trie_codec;
use models::rust::par_bag::{ParBag, ParBagCodecError, PAR_BAG_KEY_DOMAIN};
use models::rust::rhoapi_ext::EPathMap;
use pathmap::arena_compact::COMPACT_TREE_MAGIC;
use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::pathmap_codec::CompactTrie;

mod par_corpus;

const EPM1_TAG: [u8; 5] = *b"EPM1\x01";
const EPM1_VERSION_INDEX: usize = 4;

struct FormatTag {
    format: &'static str,
    tag: &'static [u8],
}

const FORMAT_TAGS: [FormatTag; 4] = [
    FormatTag {
        format: "EPM1",
        tag: &EPM1_TAG,
    },
    FormatTag {
        format: "RCI2",
        tag: &CHANNEL_IMAGE_V2_DOMAIN,
    },
    FormatTag {
        format: "ACTree03",
        tag: &COMPACT_TREE_MAGIC,
    },
    FormatTag {
        format: "RPB3",
        tag: &PAR_BAG_KEY_DOMAIN,
    },
];

fn separation_violations(table: &[FormatTag]) -> Vec<String> {
    let mut violations = Vec::new();
    for (index, first) in table.iter().enumerate() {
        for second in &table[index + 1..] {
            match (
                first.tag.starts_with(second.tag),
                second.tag.starts_with(first.tag),
            ) {
                (true, true) => violations.push(format!(
                    "{} and {} share the tag {:02x?}",
                    first.format, second.format, first.tag
                )),
                (true, false) => violations.push(format!(
                    "the tag of {} is a prefix of the tag of {}",
                    second.format, first.format
                )),
                (false, true) => violations.push(format!(
                    "the tag of {} is a prefix of the tag of {}",
                    first.format, second.format
                )),
                (false, false) => {}
            }
        }
    }
    violations
}

fn with_byte_changed(bytes: &[u8], index: usize) -> Vec<u8> {
    let mut changed = bytes.to_vec();
    changed[index] = changed[index].wrapping_add(1);
    changed
}

fn leading_domain(bytes: &[u8]) -> [u8; 4] {
    bytes[..4]
        .try_into()
        .expect("an image holds at least four bytes")
}

fn epm1_refusal(bytes: &[u8]) -> Option<String> {
    epathmap_trie_codec::decode(bytes)
        .err()
        .map(|refusal| refusal.to_string())
}

fn rci2_refusals(bytes: &[u8]) -> [Option<ChannelImageError>; 2] {
    [
        ChannelImageV2::try_from_bytes(bytes).err(),
        ValidatedChannelImage::validate_exact(bytes).err(),
    ]
}

fn act_refusal(bytes: &[u8]) -> Option<String> {
    CompactTrie::parse_canonical(bytes)
        .err()
        .map(|refusal| refusal.to_string())
}

fn rpb3_refusal(bytes: &[u8]) -> Option<ParBagCodecError> {
    ParBag::from_compact_bytes(bytes).err()
}

fn epm1_images() -> [Vec<u8>; 2] {
    let set = par_corpus::ground_pathmap().trie_snapshot().to_vec();
    let map = EPathMap::new_map(
        [
            (par_corpus::gint(1), par_corpus::gint(10)),
            (par_corpus::gint(2), par_corpus::gint(20)),
        ],
        Vec::new(),
        false,
        None,
    )
    .trie_snapshot()
    .to_vec();
    for image in [&set, &map] {
        assert!(
            image.starts_with(&EPM1_TAG),
            "an EPM1 image starts with its tag"
        );
        assert_eq!(
            epm1_refusal(image),
            None,
            "the EPM1 validator accepts the image"
        );
    }
    [set, map]
}

fn rci2_image() -> Vec<u8> {
    let image = ChannelImageV2::from_par(&par_corpus::all_par_fields())
        .expect("a resident Par has an RCI2 image")
        .as_bytes()
        .to_vec();
    assert!(
        image.starts_with(&CHANNEL_IMAGE_V2_DOMAIN),
        "an RCI2 image starts with its tag"
    );
    assert_eq!(
        rci2_refusals(&image),
        [None, None],
        "both RCI2 validators accept the image"
    );
    image
}

fn act_image_with_other_keys() -> Vec<u8> {
    let keys: [&[u8]; 3] = [
        b"raw-key-alpha-0001",
        b"raw-key-beta-0002",
        b"raw-key-gamma-0003",
    ];
    let mut bag = Bag::new();
    for key in keys {
        bag.insert(key).expect("a raw Bag accepts any byte path");
    }
    let image = bag.compact_bytes();
    assert!(
        image.starts_with(&COMPACT_TREE_MAGIC),
        "an ACTree03 image starts with its tag"
    );
    assert_eq!(
        act_refusal(&image),
        None,
        "the ACTree03 validator accepts the image"
    );
    image
}

fn rpb3_image() -> Vec<u8> {
    let image = par_corpus::all_par_fields().components().compact_bytes();
    let keys = Bag::from_compact_bytes(&image).expect("an RPB3 image is a Bag image");
    assert!(
        keys.iter().count() > 1,
        "the image holds several component keys"
    );
    assert!(
        keys.iter()
            .all(|(key, _)| key.starts_with(&PAR_BAG_KEY_DOMAIN)),
        "every key of an RPB3 image starts with its tag"
    );
    assert_eq!(
        rpb3_refusal(&image),
        None,
        "the RPB3 validator accepts the image"
    );
    image
}

fn rpb3_image_with_domain_byte_changed(image: &[u8], index: usize) -> Vec<u8> {
    let entries = Bag::from_compact_bytes(image).expect("an RPB3 image is a Bag image");
    let mut changed = Bag::new();
    for (key, multiplicity) in entries.iter() {
        changed
            .insert_many(&with_byte_changed(&key, index), multiplicity)
            .expect("a raw Bag accepts any byte path");
    }
    changed.compact_bytes()
}

#[test]
fn the_four_tags_are_distinct_and_none_is_a_prefix_of_another() {
    assert_eq!(separation_violations(&FORMAT_TAGS), Vec::<String>::new());
}

#[test]
fn the_separation_check_refuses_a_table_whose_rci2_tag_is_the_epm1_tag() {
    let mut faulted = FORMAT_TAGS;
    faulted[1].tag = &EPM1_TAG;
    assert_eq!(separation_violations(&faulted), [
        "EPM1 and RCI2 share the tag [45, 50, 4d, 31, 01]"
    ]);
}

#[test]
fn a_valid_image_with_a_changed_tag_or_version_byte_is_refused_by_its_validator() {
    for image in epm1_images() {
        for index in 0..EPM1_TAG.len() {
            let expected = match index {
                EPM1_VERSION_INDEX => "unsupported EPM1 version 2",
                _ => "invalid EPM1 magic",
            };
            assert_eq!(
                epm1_refusal(&with_byte_changed(&image, index)).as_deref(),
                Some(expected),
                "EPM1 tag byte {index}"
            );
        }
    }

    let rci2 = rci2_image();
    for index in 0..CHANNEL_IMAGE_V2_DOMAIN.len() {
        let changed = with_byte_changed(&rci2, index);
        let refusal = Some(ChannelImageError::InvalidDomain {
            found: leading_domain(&changed),
        });
        assert_eq!(
            rci2_refusals(&changed),
            [refusal.clone(), refusal],
            "RCI2 tag byte {index}"
        );
    }

    let act = act_image_with_other_keys();
    for index in 0..COMPACT_TREE_MAGIC.len() {
        assert_eq!(
            act_refusal(&with_byte_changed(&act, index)).as_deref(),
            Some("invalid ACTree03 magic"),
            "ACTree03 tag byte {index}"
        );
    }

    let rpb3 = rpb3_image();
    for index in 0..PAR_BAG_KEY_DOMAIN.len() {
        let changed = rpb3_image_with_domain_byte_changed(&rpb3, index);
        assert_eq!(
            act_refusal(&changed),
            None,
            "RPB3 domain byte {index}: still an ACTree03 image"
        );
        assert_eq!(
            rpb3_refusal(&changed),
            Some(ParBagCodecError::InvalidDomain {
                found: leading_domain(&with_byte_changed(&PAR_BAG_KEY_DOMAIN, index)),
            }),
            "RPB3 domain byte {index}"
        );
    }
}

#[test]
fn each_validator_refuses_a_valid_image_of_another_format() {
    let epm1 = epm1_images();
    let rci2 = rci2_image();
    let act = act_image_with_other_keys();
    let rpb3 = rpb3_image();

    for (format, image) in [("RCI2", &rci2), ("ACTree03", &act), ("RPB3", &rpb3)] {
        assert_eq!(
            epm1_refusal(image).as_deref(),
            Some("invalid EPM1 magic"),
            "EPM1 refuses a {format} image"
        );
    }
    for (format, image) in [
        ("EPM1 set", &epm1[0]),
        ("EPM1 map", &epm1[1]),
        ("ACTree03", &act),
        ("RPB3", &rpb3),
    ] {
        let refusal = Some(ChannelImageError::InvalidDomain {
            found: leading_domain(image),
        });
        assert_eq!(
            rci2_refusals(image),
            [refusal.clone(), refusal],
            "RCI2 refuses a {format} image"
        );
    }
    for (format, image) in [
        ("EPM1 set", &epm1[0]),
        ("EPM1 map", &epm1[1]),
        ("RCI2", &rci2),
    ] {
        assert_eq!(
            act_refusal(image).as_deref(),
            Some("invalid ACTree03 magic"),
            "ACTree03 refuses a {format} image"
        );
    }
    assert_eq!(
        act_refusal(&rpb3),
        None,
        "an RPB3 image is an ACTree03 image"
    );
    assert_eq!(
        rpb3_refusal(&act),
        Some(ParBagCodecError::InvalidDomain { found: *b"raw-" }),
        "RPB3 refuses an ACTree03 image whose keys are not RPB3 keys"
    );
}
