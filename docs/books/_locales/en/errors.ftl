# Error messages for dapple-prompt documentation
# Language: English (en)

# HTTP Status Codes
error-400 = Bad Request
error-400-desc = The request was malformed or contained invalid data.

error-401 = Unauthorized
error-401-desc = Authentication is required or the provided credentials are invalid.

error-403 = Forbidden
error-403-desc = You don't have permission to access this resource.

error-404 = Not Found
error-404-desc = The requested resource doesn't exist.

error-409 = Conflict
error-409-desc = The request conflicts with the current state (e.g., duplicate key).

error-500 = Internal Server Error
error-500-desc = An unexpected error occurred on the server.

error-503 = Service Unavailable
error-503-desc = The service is temporarily unavailable.

# Authentication Errors
error-auth-malformed-token = Malformed Token
error-auth-malformed-token-desc = The token structure is invalid.

error-auth-invalid-signature = Invalid Signature
error-auth-invalid-signature-desc = The signature verification failed.

error-auth-expired = Token Expired
error-auth-expired-desc = The token has passed its expiration time.

error-auth-not-yet-valid = Token Not Yet Valid
error-auth-not-yet-valid-desc = The current time is before the token's valid_from timestamp.

error-auth-signer-not-authorized = Signer Not Authorized
error-auth-signer-not-authorized-desc = The signer address is not in the allowlist.

error-auth-service-not-found = Service Not Found
error-auth-service-not-found-desc = No active service token found for the provided service_id.

error-auth-already-revoked = Already Revoked
error-auth-already-revoked-desc = The token has already been revoked.

# Validation Errors
error-validation-required = Required Field Missing
error-validation-required-desc = A required field was not provided.

error-validation-format = Invalid Format
error-validation-format-desc = The field value doesn't match the expected format.

error-validation-range = Value Out of Range
error-validation-range-desc = The value is outside the allowed range.
