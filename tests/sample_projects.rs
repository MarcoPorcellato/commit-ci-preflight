// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use commit_ci_preflight::config::ConfigV1;

const SAMPLES: [(&str, &str, &str); 3] = [
    (
        "rust",
        include_str!("../examples/projects/rust/.commit-ci-preflight.toml"),
        "examples/rust",
    ),
    (
        "python",
        include_str!("../examples/projects/python/.commit-ci-preflight.toml"),
        "examples/python",
    ),
    (
        "node",
        include_str!("../examples/projects/node/.commit-ci-preflight.toml"),
        "examples/node",
    ),
];

#[test]
fn clean_room_sample_plans_parse_and_pin_images() {
    for (language, source, expected_project) in SAMPLES {
        let envelope = ConfigV1::parse(source)
            .unwrap_or_else(|error| panic!("{language} sample did not parse: {error}"))
            .into_plan()
            .unwrap_or_else(|error| panic!("{language} sample did not normalize: {error}"));

        assert_eq!(envelope.plan.project, expected_project);
        assert!(envelope.plan.runtime.image.contains("@sha256:"));
        assert!(!envelope.plan.runtime.network);
        assert_eq!(envelope.plan.checks.len(), 1);
    }
}
