//  Copyright 2023 Datafuse Labs.
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.

use std::sync::Arc;

use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::TableSchemaRef;
use futures_util::future;
use opendal::Operator;
use storages_common_pruner::BlockMetaIndex;
use storages_common_table_meta::meta::BlockMeta;
use storages_common_table_meta::meta::Location;

use crate::pruning::PruningContext;

/// Segment level pruning.
pub struct SegmentPruner {
    pub pruning_ctx: PruningContext,
    pub operator: Operator,
    pub table_schema: TableSchemaRef,
}

impl SegmentPruner {
    pub fn create(
        pruning_ctx: PruningContext,
        operator: Operator,
        table_schema: TableSchemaRef,
    ) -> Result<SegmentPruner> {
        Ok(SegmentPruner {
            pruning_ctx,
            operator,
            table_schema,
        })
    }

    pub async fn pruning(
        &self,
        segment_locs: Vec<Location>,
    ) -> Result<Vec<(BlockMetaIndex, Arc<BlockMeta>)>> {
        if segment_locs.is_empty() {
            return Ok(vec![]);
        }

        // Build pruning tasks.
        let mut segments = segment_locs.into_iter().enumerate();
        let pruning_tasks = std::iter::from_fn(|| {
            // pruning tasks are executed concurrently, check if limit exceeded before proceeding
            if self.pruning_ctx.limit_pruner.exceeded() {
                None
            } else {
                segments.next().map(|(_segment_idx, _segment_location)| {
                    let pruning_ctx = self.pruning_ctx.clone();
                    move |_permit| async move { Self::prune_segment(pruning_ctx).await }
                })
            }
        });

        // Run tasks and collect the results.
        let pruning_runtime = self.pruning_ctx.pruning_runtime.clone();
        let pruning_semaphore = self.pruning_ctx.pruning_semaphore.clone();
        let join_handlers = pruning_runtime
            .try_spawn_batch_with_owned_semaphore(pruning_semaphore, pruning_tasks)
            .await?;

        let joint = future::try_join_all(join_handlers)
            .await
            .map_err(|e| ErrorCode::StorageOther(format!("segment pruning failure, {}", e)))?;

        let metas = joint
            .into_iter()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(metas)
    }

    async fn prune_segment(
        _pruning_ctx: PruningContext,
    ) -> Result<Vec<(BlockMetaIndex, Arc<BlockMeta>)>> {
        todo!()
    }
}
