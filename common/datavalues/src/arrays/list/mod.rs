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

use common_arrow::arrow::array::*;
use common_arrow::arrow::bitmap::Bitmap;
use common_exception::ErrorCode;
use common_exception::Result;

mod builder;
mod iterator;
use std::sync::Arc;

pub use builder::*;
pub use iterator::*;

use crate::prelude::*;

#[derive(Debug, Clone)]
pub struct DFListArray {
    pub(crate) array: LargeListArray,
    pub data_type: DataType,
}

impl DFListArray {
    pub fn new(array: LargeListArray) -> Self {
        let data_type = array.data_type().into();
        let data_type: DataType = data_type_physical(data_type);
        Self { array, data_type }
    }

    pub fn from_arrow_array(array: &dyn Array) -> Self {
        Self::new(
            array
                .as_any()
                .downcast_ref::<LargeListArray>()
                .unwrap()
                .clone(),
        )
    }

    pub fn data_type(&self) -> &DataType {
        &self.data_type
    }

    pub fn inner(&self) -> &LargeListArray {
        &self.array
    }

    /// # Safety
    /// Note this doesn't do any bound checking, for performance reason.
    pub unsafe fn try_get(&self, index: usize) -> Result<DataValue> {
        let v = match self.array.is_null(index) {
            true => None,
            false => {
                let netesed = self.array.value_unchecked(index);
                let netesed: ArrayRef = Arc::from(netesed);
                let netesed = netesed.into_series();
                let mut v = Vec::with_capacity(netesed.len());
                for i in 0..netesed.len() {
                    v.push(netesed.try_get(i)?);
                }
                Some(v)
            }
        };
        Ok(DataValue::List(v, self.sub_data_type().clone()))
    }

    pub fn len(&self) -> usize {
        self.array.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn null_count(&self) -> usize {
        self.array.null_count()
    }

    #[inline]
    pub fn is_null(&self, i: usize) -> bool {
        self.array.is_null(i)
    }

    #[inline]
    pub fn validity(&self) -> Option<&Bitmap> {
        self.array.validity()
    }

    /// Take a view of top n elements
    pub fn limit(&self, num_elements: usize) -> Self {
        self.slice(0, num_elements)
    }

    pub fn slice(&self, offset: usize, length: usize) -> Self {
        let array = self.array.slice(offset, length);
        Self::new(array)
    }

    /// Unpack a array to the same physical type.
    ///
    /// # Safety
    ///
    /// This is unsafe as the data_type may be uncorrect and
    /// is assumed to be correct in other unsafe code.
    pub unsafe fn unpack(&self, array: &Series) -> Result<&Self> {
        let array_trait = &**array;
        if self.data_type() == array.data_type() {
            let ca = &*(array_trait as *const dyn SeriesTrait as *const Self);
            Ok(ca)
        } else {
            Err(ErrorCode::IllegalDataType(format!(
                "cannot unpack array {:?} into matching type {:?}",
                array,
                self.data_type()
            )))
        }
    }

    pub fn sub_data_type(&self) -> &DataType {
        match self.data_type() {
            DataType::List(sub_types) => sub_types.data_type(),
            _ => unreachable!(),
        }
    }
}
