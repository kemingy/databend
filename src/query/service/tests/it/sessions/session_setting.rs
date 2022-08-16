// Copyright 2021 Datafuse Labs.
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

use common_base::base::tokio;
use common_exception::Result;
use common_settings::Settings;
use databend_query::sessions::{Session, SessionContext};
use databend_query::sessions::SessionManager;
use databend_query::sessions::SessionType;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_session_setting() -> Result<()> {
    let conf = crate::tests::ConfigBuilder::create().build();

    let id = String::from("dummy");
    let typ = SessionType::Dummy;
    let tenant = conf.query.tenant_id.clone();
    let session_settings = Settings::default_settings(&tenant);
    let session_ctx = SessionContext::try_create(conf.clone(), session_settings)?;
    let session = Session::try_create(id, typ, session_ctx, None)?;

    // Settings.
    {
        let settings = session.get_settings();
        settings.set_settings("max_threads".to_string(), "3".to_string(), true)?;
        let actual = settings.get_max_threads()?;
        let expect = 3;
        assert_eq!(actual, expect);
    }

    Ok(())
}
