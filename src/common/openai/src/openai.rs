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

pub struct OpenAI {
    pub(crate) api_key: String,
    pub(crate) api_base: String,
    pub(crate) embedding_model: String,
    pub(crate) completion_model: String,
}

impl OpenAI {
    pub fn create(
        api_base: String,
        api_key: String,
        embedding_model: String,
        completion_model: String,
    ) -> Self {
        // Check and default.
        let api_base = if api_base.is_empty() {
            "https://api.openai.com/v1/".to_string()
        } else {
            api_base
        };

        let embedding_model = if embedding_model.is_empty() {
            "text-embedding-ada-002".to_string()
        } else {
            embedding_model
        };

        let completion_model = if completion_model.is_empty() {
            "gpt-3.5-turbo".to_string()
        } else {
            completion_model
        };

        OpenAI {
            api_base,
            api_key,
            embedding_model,
            completion_model,
        }
    }
}
