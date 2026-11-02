use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{json_err, ApiError, AppState};
use crate::types::PresetSettings;

fn io_err(e: anyhow::Error) -> ApiError {
    json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
}

#[derive(Deserialize)]
pub(crate) struct CreatePreset {
    name: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)] // spec §9: all preset fields optional (sparse overlay)
    settings: PresetSettings,
}

#[derive(Deserialize)]
pub(crate) struct UpdatePreset {
    name: Option<String>,
    notes: Option<String>,
    settings: Option<PresetSettings>,
}

#[derive(Deserialize)]
pub(crate) struct ImportPreset {
    name: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    settings: serde_json::Value,
}

#[derive(Serialize)]
pub(crate) struct ExportedPreset<'a> {
    name: &'a str,
    notes: &'a str,
    settings: &'a PresetSettings,
}

pub async fn list(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let ps = st.store.list_presets().map_err(io_err)?;
    Ok(Json(serde_json::to_value(ps).unwrap()))
}

pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    match st.store.get_preset(id).map_err(io_err)? {
        Some(p) => Ok(Json(serde_json::to_value(p).unwrap())),
        None => {
            if st.store.preset_is_legacy(id).map_err(io_err)? {
                Err(json_err(
                    StatusCode::GONE,
                    "legacy preset format — recreate",
                ))
            } else {
                Err(json_err(StatusCode::NOT_FOUND, "preset not found"))
            }
        }
    }
}

pub async fn create(
    State(st): State<AppState>,
    Json(body): Json<CreatePreset>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    match st.store.create_preset(
        &body.name,
        body.notes.as_deref().unwrap_or(""),
        &body.settings,
    ) {
        Ok(p) => Ok((StatusCode::CREATED, Json(serde_json::to_value(p).unwrap()))),
        Err(e) => {
            // rusqlite renders this as "Unique constraint failed: presets.name"
            // (capital U, lowercase "nique") — match case-insensitively.
            if e.to_string().to_ascii_lowercase().contains("unique") {
                Err(json_err(StatusCode::CONFLICT, "preset name already exists"))
            } else {
                Err(io_err(e))
            }
        }
    }
}

pub async fn update(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdatePreset>,
) -> Result<Json<Value>, ApiError> {
    match st
        .store
        .update_preset(id, body.name.as_deref(), body.notes.as_deref(), body.settings.as_ref())
    {
        Ok(Some(p)) => Ok(Json(serde_json::to_value(p).unwrap())),
        Ok(None) => Err(json_err(StatusCode::NOT_FOUND, "preset not found")),
        Err(e) => {
            // rusqlite renders this as "Unique constraint failed: presets.name"
            // (capital U, lowercase "nique") — match case-insensitively.
            if e.to_string().to_ascii_lowercase().contains("unique") {
                Err(json_err(StatusCode::CONFLICT, "preset name already exists"))
            } else {
                Err(io_err(e))
            }
        }
    }
}

pub async fn delete(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if !st.store.delete_preset(id).map_err(io_err)? {
        return Err(json_err(StatusCode::NOT_FOUND, "preset not found"));
    }
    // A GPU pointed at the just-deleted preset would otherwise dangle on
    // `preset:<id>` (UI stuck on "custom", edits stop persisting). Revert it.
    crate::persist::heal_stale_preset_sources(&st.control, &st.store).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn export(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    match st.store.get_preset(id).map_err(io_err)? {
        Some(p) => Ok(Json(
            serde_json::to_value(ExportedPreset {
                name: &p.name,
                notes: &p.notes,
                settings: &p.settings,
            })
            .unwrap(),
        )),
        None => Err(json_err(StatusCode::NOT_FOUND, "preset not found")),
    }
}

/// Import = upsert by name (single shareable file, spec §8).
pub async fn import(
    State(st): State<AppState>,
    Json(body): Json<ImportPreset>,
) -> Result<Json<Value>, ApiError> {
    // Slot-map (v1) exports are rejected, not silently imported as empty
    // (spec §8.2: no backward compatibility).
    if body
        .settings
        .as_object()
        .is_some_and(|o| o.keys().any(|k| crate::types::is_pci_slot_id(k)))
    {
        return Err(json_err(
            StatusCode::BAD_REQUEST,
            "legacy slot-map settings — re-export from a current version",
        ));
    }
    let settings: PresetSettings = serde_json::from_value(body.settings)
        .map_err(|e| json_err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let existing = st
        .store
        .list_presets()
        .map_err(io_err)?
        .into_iter()
        .find(|p| p.name == body.name);
    let id = match existing {
        Some(p) => st
            .store
            .update_preset(p.id.unwrap(), None, body.notes.as_deref(), Some(&settings))
            .map_err(io_err)?
            .ok_or_else(|| json_err(StatusCode::INTERNAL_SERVER_ERROR, "preset vanished"))?
            .id
            .unwrap(),
        None => st
            .store
            .create_preset(
                &body.name,
                body.notes.as_deref().unwrap_or(""),
                &settings,
            )
            .map_err(io_err)?
            .id
            .unwrap(),
    };
    Ok(Json(serde_json::json!({ "id": id, "name": body.name })))
}

#[cfg(test)]
mod tests {
    use crate::api::tests_common;
    use crate::types::PresetSettings;
    use axum::body::Body;
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn post(app: axum::Router, uri: &str, body: Value) -> axum::response::Response {
        let req = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn preset_crud_roundtrip() {
        let st = tests_common::test_state().await;
        let app = crate::api::build_router(st);
        // create
        let res = post(
            app.clone(),
            "/api/presets",
            json!({
                "name": "quiet", "notes": "n",
                "settings": {"power_cap_w": 235.0, "sclk_offset_mhz": 100}
            }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let id = v["id"].as_i64().unwrap();
        // duplicate name -> 409
        let res = post(
            app.clone(),
            "/api/presets",
            json!({"name": "quiet", "settings": {}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CONFLICT);
        // list
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/presets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        // partial update (notes only, settings kept)
        let req = axum::http::Request::builder()
            .method("PUT")
            .uri(format!("/api/presets/{id}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"notes": "n2"}).to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(v["notes"], "n2");
        assert_eq!(v["settings"]["power_cap_w"], 235.0);
        // export
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/presets/{id}/export"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(v["name"], "quiet");
        assert!(v.get("id").is_none());
        // import under a new name
        let res = post(
            app.clone(),
            "/api/presets/import",
            json!({"name": "quiet2", "notes": "n", "settings": {"power_cap_w": 235.0}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        // delete
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri(format!("/api/presets/{id}"))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::NO_CONTENT);
        // 404 after delete
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/presets/{id}/export"))
                    .body(Body::empty())
                    .unwrap(),
            )
        .await
        .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_rename_and_collision_409() {
        let st = tests_common::test_state().await;
        let app = crate::api::build_router(st);
        let res = post(
            app.clone(),
            "/api/presets",
            json!({ "name": "r1", "settings": {"power_cap_w": 220.0} }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);
        let id: i64 = {
            let v: Value = serde_json::from_slice(
                &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
            )
            .unwrap();
            v["id"].as_i64().unwrap()
        };
        let res = post(
            app.clone(),
            "/api/presets",
            json!({ "name": "other", "settings": {} }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);

        async fn put(app: &axum::Router, id: i64, body: Value) -> axum::response::Response {
            app.clone().oneshot(
                axum::http::Request::builder()
                    .method("PUT")
                    .uri(format!("/api/presets/{id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
        }

        // Rename works.
        let res = put(&app, id, json!({ "name": "renamed" })).await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let v: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(v["name"], "renamed");
        assert_eq!(v["settings"]["power_cap_w"], 220.0);

        // Rename onto an existing name -> 409 with the create message.
        let res = put(&app, id, json!({ "name": "other" })).await;
        assert_eq!(res.status(), axum::http::StatusCode::CONFLICT);
        let v: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(v["error"], "preset name already exists");
    }

    #[tokio::test]
    async fn create_preset_empty_settings_ok() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(
            app,
            "/api/presets",
            json!({"name": "empty", "settings": {}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);
    }

    #[tokio::test]
    async fn import_rejects_slot_map() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st);
        let res = post(
            app.clone(),
            "/api/presets/import",
            json!({
                "name": "foreign",
                "settings": {"1002:75b4-1458:242f-0000:0a:00.0": {"power_cap_w": 250.0}}
            }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
        // The 400 left no row behind: the same name imports fine in v2 shape.
        let res = post(
            app,
            "/api/presets/import",
            json!({"name": "foreign", "settings": {}}),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn get_legacy_preset_returns_410() {
        let st = tests_common::test_state_full().await;
        // Insert a legacy-shape row directly into the store (test-only
        // helper — `Store.conn` is private, API tests cannot seed raw rows).
        let legacy_id = st
            .store
            .insert_raw_preset("legacy", r#"{"1002:75b4-1458:242f-0000:0b:00.0":{"power_cap_w":250.0}}"#)
            .unwrap();
        let app = crate::api::build_router(st);
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/api/presets/{legacy_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::GONE);
    }

    #[tokio::test]
    async fn list_skips_legacy_presets() {
        let st = tests_common::test_state_full().await;
        // A legacy-shape row that must be skipped.
        st.store
            .insert_raw_preset("legacy2", r#"{"1002:75b4-1458:242f-0000:0b:00.0":{"fan":{}}}"#)
            .unwrap();
        // A v2 preset that must be present — this makes the "legacy is
        // absent" assertion non-vacuous (the list is not simply empty).
        st.store
            .create_preset(
                "newshape",
                "",
                &PresetSettings {
                    power_cap_w: Some(250.0),
                    ..Default::default()
                },
            )
            .unwrap();
        let app = crate::api::build_router(st);
        let res = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/presets")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        ).unwrap();
        let arr = v.as_array().unwrap();
        // The legacy row is skipped; only new-shape presets appear.
        assert!(!arr.iter().any(|p| p["name"] == "legacy2"));
        // The new-shape preset IS present.
        assert!(arr.iter().any(|p| p["name"] == "newshape"));
    }

    #[tokio::test]
    async fn delete_preset_reverts_gpu_pointed_at_it() {
        let st = tests_common::test_state_full().await;
        let app = crate::api::build_router(st.clone());
        let slot = st.control.gpus()[0].slot.clone();

        // Create a preset and point the (only) GPU at it.
        let res = post(
            app.clone(),
            "/api/presets",
            json!({ "name": "dangling", "settings": { "power_cap_w": 200.0 } }),
        )
        .await;
        assert_eq!(res.status(), axum::http::StatusCode::CREATED);
        let v: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap(),
        )
        .unwrap();
        let id = v["id"].as_i64().unwrap();
        st.store.set_active_source(&slot, &format!("preset:{id}"));

        // Delete the preset the GPU is on.
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri(format!("/api/presets/{id}"))
            .body(Body::empty())
            .unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::NO_CONTENT);

        // The GPU's label must no longer dangle on the deleted preset — it is
        // reverted to stock (a dangling "preset:<id>" makes the UI stick on
        // "custom" and edits stop persisting).
        let sources: Value =
            serde_json::from_str(&st.store.get_meta("active_sources").unwrap().unwrap()).unwrap();
        assert_eq!(sources[&slot].as_str(), Some("stock"));
    }
}
