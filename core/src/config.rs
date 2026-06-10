use crate::database::settings_repository;
use crate::state::AppState;

/// Get a configuration value by key.
pub fn get_sys_config(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::get(&db, key)
}

/// Set a configuration value by key (upsert).
pub fn set_sys_config(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    settings_repository::set(&db, key, value)
}

#[cfg(test)]
mod config_scale_tests {
    use crate::config::{get_sys_config, set_sys_config};
    use crate::state::AppState;

    #[test]
    fn test_sys_config_massive_upserts() {
        let state =
            AppState::try_new_in_memory().expect("in-memory application state should initialize");

        // 1. Write 500 unique configuration keys and values
        for i in 0..500 {
            let key = format!("config_key_{}", i);
            let val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
            set_sys_config(&state, &key, &val).expect("Failed to set config");
        }

        // 2. Read and verify all 500 configuration keys
        for i in 0..500 {
            let key = format!("config_key_{}", i);
            let expected_val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
            let val = get_sys_config(&state, &key).expect("Failed to get config");
            assert_eq!(val, Some(expected_val));
        }

        // 3. Overwrite 250 of these keys to test upsert logic
        for i in 0..250 {
            let key = format!("config_key_{}", i);
            let new_val = format!("overwritten_value_{}", i);
            set_sys_config(&state, &key, &new_val).expect("Failed to upsert config");
        }

        // 4. Verify the overwrites and untouched config values
        for i in 0..500 {
            let key = format!("config_key_{}", i);
            let val = get_sys_config(&state, &key).expect("Failed to get config");
            if i < 250 {
                assert_eq!(val, Some(format!("overwritten_value_{}", i)));
            } else {
                let expected_val = format!("value_content_for_key_{}_{}_★_unicode_🦀", i, i * 3);
                assert_eq!(val, Some(expected_val));
            }
        }

        // 5. Test nonexistent key
        let val = get_sys_config(&state, "nonexistent_key_9999").expect("Failed to get nonexistent");
        assert_eq!(val, None);
    }
}
