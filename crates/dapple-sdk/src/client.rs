//! Core HTTP client methods for the Dapple SDK.
//!
//! These are the internal building blocks used by resource clients.
//! They handle authentication, request building, response parsing,
//! and envelope unwrapping. Implemented on `ClientCore` so both
//! `DappleClient` (internal) and `ExternalDappleClient` (external, gated
//! by the `external` feature) inherit the same machinery without
//! duplication.

use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::SdkError;
use crate::envelope::{ErrorEnvelope, GetEnvelope, IndexEnvelope};
use crate::query::bracket_serialize;

use crate::ClientCore;

impl ClientCore {
    // ========================================================================
    // GET methods
    // ========================================================================

    /// GET a single object by path. Unwraps the `{ obj }` envelope.
    pub(crate) async fn get_one<T: DeserializeOwned>(&self, path: &str) -> Result<T, SdkError> {
        let resp = self.send(Method::GET, path, None::<&()>).await?;
        let envelope: GetEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.obj)
    }

    /// GET a list of objects with query params. Unwraps the `{ objs }` envelope.
    ///
    /// Query params are serialized with bracket notation via serde_qs.
    pub(crate) async fn get_many<T: DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<Vec<T>, SdkError> {
        let qs = bracket_serialize(query)?;
        let full_path = if qs.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{qs}")
        };
        let resp = self.send(Method::GET, &full_path, None::<&()>).await?;
        let envelope: IndexEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.objs)
    }

    /// GET an endpoint that returns its body directly (no `{ obj }` /
    /// `{ objs }` envelope). Used by surfaces like
    /// `GET /api/journeys/{id}/brief-form` and `.../bindings` whose top-level
    /// shape is the response DTO itself.
    pub(crate) async fn get_flat<T: DeserializeOwned>(&self, path: &str) -> Result<T, SdkError> {
        let resp = self.send(Method::GET, path, None::<&()>).await?;
        self.parse_response(resp).await
    }

    /// GET raw bytes (for binary content like asset streaming).
    pub(crate) async fn get_bytes(&self, path: &str) -> Result<bytes::Bytes, SdkError> {
        let resp = self.send(Method::GET, path, None::<&()>).await?;
        self.check_status(&resp).await?;
        resp.bytes().await.map_err(SdkError::Http)
    }

    // ========================================================================
    // POST methods
    // ========================================================================

    /// POST with JSON body, returning a single object. Unwraps `{ obj }`.
    pub(crate) async fn post_one<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, SdkError> {
        let resp = self.send(Method::POST, path, Some(body)).await?;
        let envelope: GetEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.obj)
    }

    /// POST with JSON body, returning a list. Unwraps `{ objs }`.
    pub(crate) async fn post_many<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Vec<T>, SdkError> {
        let resp = self.send(Method::POST, path, Some(body)).await?;
        let envelope: IndexEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.objs)
    }

    /// POST a multipart form (for tarball ingest). Parses the JSON response.
    ///
    /// Builds its request inline rather than via [`Self::send`] because
    /// `reqwest::multipart::Form` is consumed on send and is not `Clone`, so it
    /// cannot be auto-retried on a 401. A 401 here still clears the cached
    /// credential (via `parse_response`), so the *next* ingest call re-exchanges.
    pub(crate) async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T, SdkError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), path);
        let auth = self.auth.authorization_header().await?;

        let resp = self
            .http
            .post(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await
            .map_err(SdkError::Http)?;

        self.parse_response(resp).await
    }

    /// POST with JSON body, returning the raw JSON value without envelope unwrapping.
    ///
    /// Used for non-standard endpoints (e.g., API tokens) that don't follow
    /// the standard `{ status, objs }` or `{ status, obj }` envelope pattern.
    pub(crate) async fn post_raw<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<serde_json::Value, SdkError> {
        let resp = self.send(Method::POST, path, Some(body)).await?;
        self.parse_response(resp).await
    }

    /// POST returning raw bytes (for PDF render).
    pub(crate) async fn post_bytes<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<bytes::Bytes, SdkError> {
        let resp = self.send(Method::POST, path, Some(body)).await?;
        self.check_status(&resp).await?;
        resp.bytes().await.map_err(SdkError::Http)
    }

    // ========================================================================
    // PATCH methods
    // ========================================================================

    /// PATCH with JSON body, returning a list. Unwraps `{ objs }`.
    pub(crate) async fn patch_many<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Vec<T>, SdkError> {
        let resp = self.send(Method::PATCH, path, Some(body)).await?;
        let envelope: IndexEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.objs)
    }

    /// PATCH a single object by path with a JSON body. Unwraps `{ obj }`.
    pub(crate) async fn patch_one<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, SdkError> {
        let resp = self.send(Method::PATCH, path, Some(body)).await?;
        let envelope: GetEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.obj)
    }

    /// PATCH a multipart form (for version ingest). Parses the JSON response.
    ///
    /// Like [`Self::post_multipart`], builds its request inline (the form is not
    /// `Clone`-able) so it cannot auto-retry on a 401; the cache is still
    /// cleared so the next call re-exchanges.
    pub(crate) async fn patch_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T, SdkError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), path);
        let auth = self.auth.authorization_header().await?;

        let resp = self
            .http
            .patch(&url)
            .header("Authorization", auth)
            .multipart(form)
            .send()
            .await
            .map_err(SdkError::Http)?;

        self.parse_response(resp).await
    }

    // ========================================================================
    // DELETE methods
    // ========================================================================

    /// DELETE a single object by path. Unwraps `{ obj }`.
    pub(crate) async fn delete_one<T: DeserializeOwned>(&self, path: &str) -> Result<T, SdkError> {
        let resp = self.send(Method::DELETE, path, None::<&()>).await?;
        let envelope: GetEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.obj)
    }

    /// DELETE with query params, returning a list. Unwraps `{ objs }`.
    pub(crate) async fn delete_many<T: DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<Vec<T>, SdkError> {
        let qs = bracket_serialize(query)?;
        let full_path = if qs.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{qs}")
        };
        let resp = self.send(Method::DELETE, &full_path, None::<&()>).await?;
        let envelope: IndexEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.objs)
    }

    /// DELETE with a JSON body, returning a list. Unwraps `{ objs }`.
    pub(crate) async fn delete_many_with_body<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<Vec<T>, SdkError> {
        let resp = self.send(Method::DELETE, path, Some(body)).await?;
        let envelope: IndexEnvelope<T> = self.parse_response(resp).await?;
        Ok(envelope.objs)
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Build and send an authenticated HTTP request, transparently
    /// re-authenticating once on a `401`.
    ///
    /// A `401` means the cached credential (e.g. a service-token JWT) is no
    /// longer accepted — most commonly because it crossed its expiry. We clear
    /// the cache via [`AuthProvider::on_unauthorized`], re-exchange a fresh
    /// credential, and resend the request exactly once so transient expiry is
    /// invisible to the caller.
    ///
    /// Retrying is safe even for non-idempotent methods: a `401` is raised by
    /// the server's auth extractor *before* the handler runs, so the first
    /// attempt had no side effect.
    ///
    /// NOTE: the multipart helpers (`post_multipart` / `patch_multipart`) build
    /// their requests inline and do NOT route through here, because
    /// `reqwest::multipart::Form` is consumed on send and is not `Clone`. They
    /// still benefit from the cache invalidation in `parse_response`, so the
    /// *next* call after a 401 re-exchanges — they just don't auto-retry the
    /// failing call itself.
    async fn send<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<reqwest::Response, SdkError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), path);

        let auth = self.auth.authorization_header().await?;
        let resp = self.send_once(&method, &url, body, auth).await?;

        if resp.status().as_u16() == 401 {
            self.auth.on_unauthorized().await;
            let auth = self.auth.authorization_header().await?;
            return self.send_once(&method, &url, body, auth).await;
        }

        Ok(resp)
    }

    /// Issue a single authenticated request with a pre-resolved Authorization
    /// header. The retry loop lives in [`Self::send`].
    async fn send_once<B: Serialize>(
        &self,
        method: &Method,
        url: &str,
        body: Option<&B>,
        auth: String,
    ) -> Result<reqwest::Response, SdkError> {
        let mut builder = self
            .http
            .request(method.clone(), url)
            .header("Authorization", auth);

        if let Some(body) = body {
            builder = builder.json(body);
        }

        builder.send().await.map_err(SdkError::Http)
    }

    /// Check the HTTP status code and return an error for non-success statuses.
    ///
    /// On a `401`, clear the cached credential so the next request
    /// re-exchanges — the cache invalidation must depend only on the status
    /// code, never on the error-body shape.
    async fn check_status(&self, resp: &reqwest::Response) -> Result<(), SdkError> {
        if resp.status().is_success() {
            Ok(())
        } else {
            if resp.status().as_u16() == 401 {
                self.auth.on_unauthorized().await;
            }
            Err(SdkError::Api {
                status: resp.status().as_u16(),
                message: format!("HTTP {}", resp.status()),
            })
        }
    }

    /// Parse a response body as JSON, handling error envelopes.
    async fn parse_response<T: DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T, SdkError> {
        let status = resp.status();
        let body = resp.text().await.map_err(SdkError::Http)?;

        if !status.is_success() {
            // A 401 means the cached credential is no longer accepted — clear
            // it so the next request re-exchanges, regardless of the
            // error-body shape. `send` also auto-retries once; this branch
            // additionally covers the inline-built multipart paths that don't
            // route through `send`.
            if status.as_u16() == 401 {
                self.auth.on_unauthorized().await;
            }
            // Surface the clean message when the body matches a known error
            // envelope; otherwise fall back to the raw body.
            if let Ok(err) = serde_json::from_str::<ErrorEnvelope>(&body) {
                return Err(SdkError::Api {
                    status: status.as_u16(),
                    message: err.message,
                });
            }
            return Err(SdkError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        serde_json::from_str(&body).map_err(|e| {
            SdkError::Deserialization(format!("{e}: {}", &body[..body.len().min(200)]))
        })
    }
}
