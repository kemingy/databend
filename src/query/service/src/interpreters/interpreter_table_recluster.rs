// Copyright 2022 Datafuse Labs.
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

use std::sync::Arc;

use common_base::base::GlobalIORuntime;
use common_exception::Result;
use common_planners::ReclusterTablePlan;
use common_streams::DataBlockStream;
use common_streams::SendableDataBlockStream;

use crate::interpreters::Interpreter;
use crate::pipelines::executor::ExecutorSettings;
use crate::pipelines::executor::PipelineCompleteExecutor;
use crate::pipelines::Pipeline;
use crate::sessions::QueryContext;
use crate::sessions::TableContext;

pub struct ReclusterTableInterpreter {
    ctx: Arc<QueryContext>,
    plan: ReclusterTablePlan,
}

impl ReclusterTableInterpreter {
    pub fn try_create(ctx: Arc<QueryContext>, plan: ReclusterTablePlan) -> Result<Self> {
        Ok(Self { ctx, plan })
    }
}

#[async_trait::async_trait]
impl Interpreter for ReclusterTableInterpreter {
    fn name(&self) -> &str {
        "ReclusterTableInterpreter"
    }

    async fn execute(&self) -> Result<SendableDataBlockStream> {
        let plan = &self.plan;
        let ctx = self.ctx.clone();
        let settings = ctx.get_settings();
        let tenant = ctx.get_tenant();
        loop {
            let table = self
                .ctx
                .get_catalog(&plan.catalog)?
                .get_table(tenant.as_str(), &plan.database, &plan.table)
                .await?;

            let mut pipeline = Pipeline::create();
            let mutator = table
                .recluster(
                    ctx.clone(),
                    plan.catalog.clone(),
                    &mut pipeline,
                    plan.push_downs.clone(),
                )
                .await?;
            let mutator = if let Some(mutator) = mutator {
                mutator
            } else {
                break;
            };

            pipeline.set_max_threads(settings.get_max_threads()? as usize);

            let async_runtime = GlobalIORuntime::instance();
            let query_need_abort = ctx.query_need_abort();
            let executor_settings = ExecutorSettings::try_create(&settings)?;
            let executor = PipelineCompleteExecutor::try_create(
                async_runtime,
                query_need_abort,
                pipeline,
                executor_settings,
            )?;
            executor.execute()?;
            drop(executor);

            let catalog_name = ctx.get_current_catalog();
            mutator
                .try_commit(&catalog_name, table.get_table_info())
                .await?;

            if !plan.is_final {
                break;
            }
        }

        Ok(Box::pin(DataBlockStream::create(
            self.plan.schema(),
            None,
            vec![],
        )))
    }
}