# Web Session Tokens (NextAuth.js)

> **Audience:** Frontend Developers · Product

Web session tokens provide browser-based authentication through NextAuth.js OAuth providers.

---

<!-- ANCHOR: when-to-use -->
## When to Use

- **Browser-based applications** where users log in via OAuth providers (Google, GitHub, etc.)
- **Session-based interactions** where the user is actively using the web interface
- **User-owned actions** where the user is directly operating the UI
<!-- ANCHOR_END: when-to-use -->

---

<!-- ANCHOR: how-it-works -->
## How It Works

1. User authenticates via NextAuth.js (OAuth provider like Google, GitHub)
2. NextAuth creates a session and issues a JWT
3. Web application includes the JWT in `Authorization: Bearer <token>` header for API calls
4. JWT expires when session ends or user logs out
<!-- ANCHOR_END: how-it-works -->

---

<!-- ANCHOR: jwt-claims -->
## JWT Claims

NextAuth.js handles the entire flow. The API validates the JWT and extracts user identity from the `sub` (subject) claim.

```json
{
  "sub": "user-uuid",
  "service_id": "web",
  "token_use": "session",
  "auth_user_id": "user-uuid",
  "roles": ["user"],
  "exp": 1705312200
}
```

| Claim | Description |
|-------|-------------|
| `sub` | User's unique identifier |
| `service_id` | Always "web" for session tokens |
| `token_use` | "session" for web sessions |
| `auth_user_id` | User ID from auth system |
| `roles` | User's roles (e.g., ["user"]) |
| `exp` | Expiration timestamp |
<!-- ANCHOR_END: jwt-claims -->

---

<!-- ANCHOR: security -->
## Security Considerations

- Session tokens are tied to browser sessions
- Automatically expire on logout
- Cannot be used outside the browser context
- No manual rotation required
- Rate limit: 300 requests/minute
<!-- ANCHOR_END: security -->
