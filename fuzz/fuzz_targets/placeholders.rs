//! The runner's placeholder filling on any template and any values: a template is filled or says
//! which placeholder has no value, never a panic; filled whole, it reads the same as a lab's
//! message does; and a value is put in as it is, never read as a placeholder itself.

#![no_main]

use std::collections::BTreeSet;

use arbitrary::Arbitrary;
use isothrum_runner::fuzzing::Values;
use isothrum_runner::isoloom_manifest::{Part, Placeholder, RunVar, Template};
use libfuzzer_sys::fuzz_target;

/// A template's text and the values its placeholders may take.
#[derive(Debug, Arbitrary)]
struct Input {
    template: String,
    name: Option<String>,
    params: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
    published: Vec<(String, String, u16)>,
    machine: Option<String>,
    value: String,
}

fuzz_target!(|input: Input| {
    let mut values = Values::default();
    if let Some(name) = &input.name {
        values.run(RunVar::Name, name);
    }
    for (name, value) in &input.params {
        values.param(name, value);
    }
    for (name, value) in &input.outputs {
        values.output(name, value);
    }
    for (machine, port, host_port) in &input.published {
        values.published(machine, port, *host_port);
    }
    if let Some(machine) = &input.machine {
        values.machine(machine);
    }
    if let Ok(template) = Template::parse(&input.template) {
        let lenient = values.render_lenient(&template, &BTreeSet::new());
        if let Ok(filled) = values.render(&template) {
            assert_eq!(filled, lenient, "a template filled whole reads the same as a message");
            // Filled without a `${value}`: the value changes nothing.
            assert_eq!(values.render_value(&template, &input.value).ok(), Some(filled));
        }
    }
    // The last value bound to a param is what its placeholder gives, as it is (a name that reads
    // as that param's placeholder alone: `a}b` would be `a`'s, then text).
    if let Some((name, _)) = input.params.last()
        && let Ok(single) = Template::parse(&format!("${{params.{name}}}"))
        && matches!(single.parts(), [Part::Placeholder(Placeholder::Param(p))] if p.as_str() == name)
    {
        let last = input.params.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.clone());
        assert_eq!(values.render(&single).ok(), last, "a value is never read as a placeholder");
    }
});
