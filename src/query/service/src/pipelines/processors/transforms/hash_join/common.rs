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

use std::collections::HashSet;

use common_arrow::arrow::bitmap::Bitmap;
use common_arrow::arrow::bitmap::MutableBitmap;
use common_catalog::table_context::TableContext;
use common_datablocks::DataBlock;
use common_datavalues::combine_validities_3;
use common_datavalues::BooleanColumn;
use common_datavalues::BooleanType;
use common_datavalues::Column;
use common_datavalues::ColumnRef;
use common_datavalues::ConstColumn;
use common_datavalues::DataField;
use common_datavalues::DataSchema;
use common_datavalues::DataSchemaRef;
use common_datavalues::NullableColumn;
use common_datavalues::NullableType;
use common_datavalues::Series;
use common_exception::ErrorCode;
use common_exception::Result;
use common_hashtable::HashMap;
use common_hashtable::HashTableKeyable;
use common_hashtable::KeyValueEntity;

use crate::evaluator::EvalNode;
use crate::pipelines::processors::transforms::hash_join::desc::MarkerKind;
use crate::pipelines::processors::transforms::hash_join::row::RowPtr;
use crate::pipelines::processors::JoinHashTable;
use crate::sql::plans::JoinType;

/// Some common methods for hash join.
impl JoinHashTable {
    // Merge build block and probe block that have the same number of rows
    pub(crate) fn merge_eq_block(
        &self,
        build_block: &DataBlock,
        probe_block: &DataBlock,
    ) -> Result<DataBlock> {
        let mut probe_block = probe_block.clone();
        for (col, field) in build_block
            .columns()
            .iter()
            .zip(build_block.schema().fields().iter())
        {
            probe_block = probe_block.add_column(col.clone(), field.clone())?;
        }
        Ok(probe_block)
    }

    #[inline]
    pub(crate) fn probe_key<Key: HashTableKeyable>(
        &self,
        hash_table: &HashMap<Key, Vec<RowPtr>>,
        key: Key,
        valids: &Option<Bitmap>,
        i: usize,
    ) -> Option<*mut KeyValueEntity<Key, Vec<RowPtr>>> {
        if valids.as_ref().map_or(true, |v| v.get_bit(i)) {
            return hash_table.find_key(&key);
        }
        None
    }

    pub(crate) fn create_marker_block(
        &self,
        has_null: bool,
        markers: Vec<MarkerKind>,
    ) -> Result<DataBlock> {
        let mut validity = MutableBitmap::with_capacity(markers.len());
        let mut boolean_bit_map = MutableBitmap::with_capacity(markers.len());

        for m in markers {
            let marker = if m == MarkerKind::False && has_null {
                MarkerKind::Null
            } else {
                m
            };
            if marker == MarkerKind::Null {
                validity.push(false);
            } else {
                validity.push(true);
            }
            if marker == MarkerKind::True {
                boolean_bit_map.push(true);
            } else {
                boolean_bit_map.push(false);
            }
        }
        let boolean_column = BooleanColumn::from_arrow_data(boolean_bit_map.into());
        let marker_column = Self::set_validity(&boolean_column.arc(), &validity.into())?;
        let marker_schema = DataSchema::new(vec![DataField::new(
            &self
                .hash_join_desc
                .marker_join_desc
                .marker_index
                .ok_or_else(|| ErrorCode::LogicalError("Invalid mark join"))?
                .to_string(),
            NullableType::new_impl(BooleanType::new_impl()),
        )]);
        Ok(DataBlock::create(DataSchemaRef::from(marker_schema), vec![
            marker_column,
        ]))
    }

    pub(crate) fn init_markers(cols: &[ColumnRef], num_rows: usize) -> Vec<MarkerKind> {
        let mut markers = vec![MarkerKind::False; num_rows];
        if cols.iter().any(|c| c.is_nullable() || c.is_null()) {
            let mut valids = None;
            for col in cols.iter() {
                let (is_all_null, tmp_valids_option) = col.validity();
                if !is_all_null {
                    if let Some(tmp_valids) = tmp_valids_option.as_ref() {
                        if tmp_valids.unset_bits() == 0 {
                            let mut m = MutableBitmap::with_capacity(num_rows);
                            m.extend_constant(num_rows, true);
                            valids = Some(m.into());
                            break;
                        } else {
                            valids = combine_validities_3(valids, tmp_valids_option.cloned());
                        }
                    }
                }
            }
            if let Some(v) = valids {
                for (idx, marker) in markers.iter_mut().enumerate() {
                    if !v.get_bit(idx) {
                        *marker = MarkerKind::Null;
                    }
                }
            }
        }
        markers
    }

    pub(crate) fn set_validity(column: &ColumnRef, validity: &Bitmap) -> Result<ColumnRef> {
        if column.is_null() {
            Ok(column.clone())
        } else if column.is_const() {
            let col: &ConstColumn = Series::check_get(column)?;
            let validity = validity.clone();
            let inner = Self::set_validity(col.inner(), &validity.slice(0, 1))?;
            Ok(ConstColumn::new(inner, col.len()).arc())
        } else if column.is_nullable() {
            let col: &NullableColumn = Series::check_get(column)?;
            // It's possible validity is longer than col.
            let diff_len = validity.len() - col.ensure_validity().len();
            let mut new_validity = MutableBitmap::with_capacity(validity.len());
            for (b1, b2) in validity.iter().zip(col.ensure_validity().iter()) {
                new_validity.push(b1 & b2);
            }
            new_validity.extend_constant(diff_len, false);
            let col = NullableColumn::wrap_inner(col.inner().clone(), Some(new_validity.into()));
            Ok(col)
        } else {
            let col = NullableColumn::wrap_inner(column.clone(), Some(validity.clone()));
            Ok(col)
        }
    }

    // return an (option bitmap, all_true, all_false)
    pub(crate) fn get_other_filters(
        &self,
        merged_block: &DataBlock,
        filter: &EvalNode,
    ) -> Result<(Option<Bitmap>, bool, bool)> {
        let func_ctx = self.ctx.try_get_function_context()?;
        // `predicate_column` contains a column, which is a boolean column.
        let filter_vector = filter.eval(&func_ctx, merged_block)?;
        let predict_boolean_nonull = DataBlock::cast_to_nonull_boolean(filter_vector.vector())?;

        // faster path for constant filter
        if predict_boolean_nonull.is_const() {
            let v = predict_boolean_nonull.get_bool(0)?;
            return Ok((None, v, !v));
        }

        let boolean_col: &BooleanColumn = Series::check_get(&predict_boolean_nonull)?;
        let rows = boolean_col.len();
        let count_zeros = boolean_col.values().unset_bits();

        Ok((
            Some(boolean_col.values().clone()),
            count_zeros == 0,
            rows == count_zeros,
        ))
    }

    pub(crate) fn find_unmatched_build_indexes(&self) -> Result<Vec<RowPtr>> {
        // For right/full join, build side will appear at least once in the joined table
        // Find the unmatched rows in build side
        let mut unmatched_build_indexes = vec![];
        let build_indexes = self.hash_join_desc.right_join_desc.build_indexes.read();
        let build_indexes_set: HashSet<&RowPtr> = build_indexes.iter().collect();
        // TODO(xudong): remove the line of code below after https://github.com/rust-lang/rust-clippy/issues/8987
        #[allow(clippy::significant_drop_in_scrutinee)]
        for (chunk_index, chunk) in self.row_space.chunks.read().unwrap().iter().enumerate() {
            for row_index in 0..chunk.num_rows() {
                let row_ptr = RowPtr {
                    chunk_index: chunk_index as u32,
                    row_index: row_index as u32,
                    marker: None,
                };
                if !build_indexes_set.contains(&row_ptr) {
                    let mut row_state = self.hash_join_desc.right_join_desc.row_state.write();
                    row_state.entry(row_ptr).or_insert(0_usize);
                    unmatched_build_indexes.push(row_ptr);
                }
                if self.hash_join_desc.join_type == JoinType::Full {
                    if let Some(row_ptr) = build_indexes_set.get(&row_ptr) {
                        // If `marker` == `MarkerKind::False`, it means the row in build side has been filtered in left probe phase
                        if row_ptr.marker == Some(MarkerKind::False) {
                            unmatched_build_indexes.push(**row_ptr);
                        }
                    }
                }
            }
        }
        Ok(unmatched_build_indexes)
    }
}