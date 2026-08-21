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

use commit_ci_preflight::agent_session::{
    AgentSessionIdentity, CapabilitySource, SessionInspector, SessionObservation, digest_capability,
};

struct FixedInspector(SessionObservation);

impl SessionInspector for FixedInspector {
    fn observe(&self, _identity: &AgentSessionIdentity) -> SessionObservation {
        self.0.clone()
    }
}

struct FixedCapability([u8; 32]);

impl CapabilitySource for FixedCapability {
    fn capability_32(
        &self,
    ) -> Result<[u8; 32], commit_ci_preflight::agent_session::AgentSessionError> {
        Ok(self.0)
    }
}

fn identity() -> AgentSessionIdentity {
    AgentSessionIdentity {
        parent_pid: 42,
        parent_start: "Wed Aug 21 10:00:00 2026".to_owned(),
        boot_id: "boot-opaque-1".to_owned(),
    }
}

#[test]
fn injected_observations_remain_distinct_and_deterministic() {
    for observation in [
        SessionObservation::Live,
        SessionObservation::LostParent,
        SessionObservation::Reparented,
        SessionObservation::Rebooted,
        SessionObservation::Ambiguous,
        SessionObservation::Unsupported,
    ] {
        assert_eq!(
            FixedInspector(observation.clone()).observe(&identity()),
            observation
        );
    }
}

#[test]
fn capability_is_available_to_its_caller_but_only_digest_is_serialized() {
    let capability = FixedCapability([0; 32])
        .capability_32()
        .expect("capability");

    assert_eq!(
        digest_capability(&capability),
        "sha256:6a99e1eb72d896ab541f7846f287968783ebf0d2faaa7324b3c11b36f4ab060e"
    );
}
