//! M9 durability acceptance: autosave/recovery round-trips and keyframe-
//! preserving parameter writes — the two ways this slice protects user work.
//!
//! Nothing here touches the UI: the autosave module and the composition write
//! are the exact functions the shell calls.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::HashMap;
use std::time::Duration;

use filmcraft_project::Project;
use filmcraft_project::keyframe::{Param, ParamValue};
use filmcraft_time::{TICKS_PER_SECOND, Tick};
use frameforge_bridge::{Composition, EffectLink, FrameForgeProject, autosave};

fn doc() -> FrameForgeProject {
    FrameForgeProject::new(Project::new("Durability"))
}

#[test]
fn autosave_round_trip_newer_than_and_clear() {
    let dir = std::env::temp_dir();
    let project = dir.join(format!("frameforge-m9-autosave-{}.ffproj", std::process::id()));
    let autosave_path = autosave::autosave_path(&project);
    let document = doc();

    document.save(&project).expect("save the project");
    // Filesystem timestamps are coarse on some systems; make "newer" unambiguous.
    std::thread::sleep(Duration::from_millis(1_100));
    let written = autosave::write(&document, &project).expect("write the autosave");
    assert_eq!(written, autosave_path, "the sidecar sits next to the project");
    assert_eq!(autosave::newer_than(&project), Some(autosave_path.clone()), "a newer autosave is detected");

    let loaded = autosave::load(&project).expect("load the autosave");
    assert_eq!(
        serde_json::to_value(&loaded).expect("serialize loaded"),
        serde_json::to_value(&document).expect("serialize original"),
        "the autosave must be the same document"
    );

    autosave::clear(&project).expect("clear the autosave");
    assert!(autosave::newer_than(&project).is_none(), "a cleared autosave is not offered");
    assert!(!autosave_path.exists(), "the sidecar file is gone");
    autosave::clear(&project).expect("clearing a missing autosave is success");
}

#[test]
fn corrupt_autosave_is_a_clear_error_not_a_crash() {
    let dir = std::env::temp_dir();
    let project = dir.join(format!("frameforge-m9-corrupt-{}.ffproj", std::process::id()));
    doc().save(&project).expect("save the project");
    std::thread::sleep(Duration::from_millis(1_100));
    let autosave_path = autosave::autosave_path(&project);
    std::fs::write(&autosave_path, b"{ this is not a project").expect("write garbage");

    // The recovery candidate is still reported (the UI must be able to say the
    // file is unreadable); loading it is a value, never a panic.
    assert_eq!(autosave::newer_than(&project), Some(autosave_path), "the unreadable sidecar is still newer");
    assert!(autosave::load(&project).is_err(), "a corrupt autosave is an error the UI can report");
    autosave::clear(&project).expect("clear the corrupt sidecar");
}

#[test]
fn scalar_writes_preserve_keyframes() {
    let mut composition = Composition::default();
    let mut animated = Param::new(ParamValue::Float(0.0));
    animated.put_keyframe(Tick::ZERO, ParamValue::Float(0.0));
    animated.put_keyframe(Tick(TICKS_PER_SECOND), ParamValue::Float(10.0));
    composition.effects.push(EffectLink {
        effect: "ec.blur.gaussian".to_string(),
        enabled: true,
        params: HashMap::from([("blurriness".to_string(), animated)]),
    });

    // The user drags the slider at 0.5 s: the animation must survive and gain
    // a key there, not be replaced by a static value.
    assert!(composition.set_scalar(0, "blurriness", Tick(TICKS_PER_SECOND / 2), 3.0));
    let param = &composition.effects[0].params["blurriness"];
    assert_eq!(param.keyframes.len(), 3, "the two keys stay and a third is added");
    assert!((param.value_at(Tick(TICKS_PER_SECOND / 2)).as_f64().unwrap() - 3.0).abs() < 1e-9);
    assert!((param.value_at(Tick(TICKS_PER_SECOND)).as_f64().unwrap() - 10.0).abs() < 1e-9, "the later key is untouched");

    // A static parameter stays static: the sidebar value is replaced in place.
    let mut static_comp = Composition::default();
    static_comp.effects.push(EffectLink {
        effect: "ec.blur.gaussian".to_string(),
        enabled: true,
        params: HashMap::from([("blurriness".to_string(), Param::new(ParamValue::Float(1.0)))]),
    });
    assert!(static_comp.set_scalar(0, "blurriness", Tick(TICKS_PER_SECOND), 9.0));
    let param = &static_comp.effects[0].params["blurriness"];
    assert!(!param.is_animated(), "a static write does not invent keyframes");
    assert!((param.value.as_f64().unwrap() - 9.0).abs() < 1e-9);

    // A parameter that has never been written is created with the new value.
    let mut empty = Composition::default();
    empty.effects.push(EffectLink { effect: "ec.blur.gaussian".to_string(), enabled: true, params: HashMap::new() });
    assert!(empty.set_scalar(0, "blurriness", Tick::ZERO, 4.0));
    assert!((empty.effects[0].params["blurriness"].value.as_f64().unwrap() - 4.0).abs() < 1e-9);

    // Out-of-range indices are a value, not a panic.
    assert!(!empty.set_scalar(7, "blurriness", Tick::ZERO, 1.0));
}
