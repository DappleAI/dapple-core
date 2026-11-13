# Plan: demo-studio API Client Implementation

## Objective

Wire up demo-studio (dapple-ui) to connect exclusively to dapple-prompt-api following the patterns defined in plans 004 and 005.

## Prerequisites

- Plan 004: demo-studio single connection point
- Plan 005: TTS normalization via prompt system
- dapple-prompt-api running with:
  - `/api/get-auth-token` endpoint
  - `/api/assets/{id}/content` endpoint
  - Dispatch endpoints

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  demo-studio (dapple-ui)                                        │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  AuthProvider                                            │   │
│  │  - Signs token with user's private key                   │   │
│  │  - Exchanges for JWT via /api/get-auth-token             │   │
│  │  - Stores JWT, handles refresh                           │   │
│  └─────────────────────────────────────────────────────────┘   │
│                            │                                    │
│                            ▼                                    │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  ApiClient                                               │   │
│  │  - Base URL: DAPPLE_API_URL                              │   │
│  │  - Attaches JWT to all requests                          │   │
│  │  - Methods: dispatch(), getAssetContent(), etc.          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                            │                                    │
└────────────────────────────┼────────────────────────────────────┘
                             │ HTTP
                             ▼
                  ┌─────────────────────┐
                  │  dapple-prompt-api  │
                  └─────────────────────┘
```

## Implementation

### 1. Environment Configuration

```typescript
// lib/config.ts

export const config = {
  // ONLY URL needed - everything goes through the API
  apiUrl: process.env.NEXT_PUBLIC_DAPPLE_API_URL || 'http://localhost:8080',
};

// Remove any of these if they exist:
// - CONDUIT_URL
// - DATABASE_URL
// - REDIS_URL
// - TTS_*
```

### 2. Auth Provider

```typescript
// providers/AuthProvider.tsx

import { createContext, useContext, useState, useCallback } from 'react';
import { ethers } from 'ethers';

interface AuthContextType {
  jwt: string | null;
  address: string | null;
  isAuthenticated: boolean;
  login: () => Promise<void>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [jwt, setJwt] = useState<string | null>(null);
  const [address, setAddress] = useState<string | null>(null);

  const login = useCallback(async () => {
    // 1. Get signer from wallet
    const provider = new ethers.BrowserProvider(window.ethereum);
    const signer = await provider.getSigner();
    const userAddress = await signer.getAddress();

    // 2. Create auth token payload
    const payload = {
      address: userAddress,
      timestamp: Date.now(),
      nonce: crypto.randomUUID(),
    };

    // 3. Sign the payload
    const message = JSON.stringify(payload);
    const signature = await signer.signMessage(message);

    // 4. Exchange for JWT
    const response = await fetch(`${config.apiUrl}/api/get-auth-token`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        payload,
        signature,
      }),
    });

    if (!response.ok) {
      throw new Error('Authentication failed');
    }

    const { token } = await response.json();

    setJwt(token);
    setAddress(userAddress);

    // Store in localStorage for persistence
    localStorage.setItem('dapple_jwt', token);
    localStorage.setItem('dapple_address', userAddress);
  }, []);

  const logout = useCallback(() => {
    setJwt(null);
    setAddress(null);
    localStorage.removeItem('dapple_jwt');
    localStorage.removeItem('dapple_address');
  }, []);

  return (
    <AuthContext.Provider value={{
      jwt,
      address,
      isAuthenticated: !!jwt,
      login,
      logout,
    }}>
      {children}
    </AuthContext.Provider>
  );
}

export const useAuth = () => {
  const context = useContext(AuthContext);
  if (!context) throw new Error('useAuth must be used within AuthProvider');
  return context;
};
```

### 3. API Client

```typescript
// lib/api-client.ts

import { config } from './config';

export class ApiClient {
  private jwt: string | null = null;

  setJwt(jwt: string | null) {
    this.jwt = jwt;
  }

  private async fetch(path: string, options: RequestInit = {}) {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...options.headers as Record<string, string>,
    };

    if (this.jwt) {
      headers['Authorization'] = `Bearer ${this.jwt}`;
    }

    const response = await fetch(`${config.apiUrl}${path}`, {
      ...options,
      headers,
    });

    if (!response.ok) {
      throw new ApiError(response.status, await response.text());
    }

    return response;
  }

  // =========================================================================
  // Journeys
  // =========================================================================

  async getJourneys() {
    const response = await this.fetch('/api/journeys');
    return response.json();
  }

  async getJourney(objectId: string) {
    const response = await this.fetch(`/api/journeys/${objectId}`);
    return response.json();
  }

  // =========================================================================
  // Dispatch
  // =========================================================================

  async triggerDispatch(journeyObjectId: string, stepObjectId: string) {
    const response = await this.fetch('/api/dispatch', {
      method: 'POST',
      body: JSON.stringify({
        journey_object_id: journeyObjectId,
        step_object_id: stepObjectId,
      }),
    });
    return response.json();
  }

  async getDispatchResult(dispatchObjectId: string) {
    const response = await this.fetch(`/api/dispatch/${dispatchObjectId}`);
    return response.json();
  }

  // =========================================================================
  // Assets
  // =========================================================================

  async getAsset(objectId: string) {
    const response = await this.fetch(`/api/assets/${objectId}`);
    return response.json();
  }

  /**
   * Get asset content as a streaming response.
   * Use for audio playback, image display, file downloads.
   */
  async getAssetContent(objectId: string): Promise<Response> {
    return this.fetch(`/api/assets/${objectId}/content`, {
      headers: {
        // Don't set Content-Type for streaming response
      },
    });
  }

  /**
   * Get asset content as a Blob for use with URL.createObjectURL()
   */
  async getAssetBlob(objectId: string): Promise<Blob> {
    const response = await this.getAssetContent(objectId);
    return response.blob();
  }

  /**
   * Get asset content as an object URL for direct use in <audio> or <img> src
   */
  async getAssetUrl(objectId: string): Promise<string> {
    const blob = await this.getAssetBlob(objectId);
    return URL.createObjectURL(blob);
  }
}

export class ApiError extends Error {
  constructor(public status: number, public body: string) {
    super(`API Error ${status}: ${body}`);
  }
}

// Singleton instance
export const apiClient = new ApiClient();
```

### 4. React Hooks

```typescript
// hooks/useApi.ts

import { useEffect } from 'react';
import { useAuth } from '../providers/AuthProvider';
import { apiClient } from '../lib/api-client';

export function useApi() {
  const { jwt } = useAuth();

  useEffect(() => {
    apiClient.setJwt(jwt);
  }, [jwt]);

  return apiClient;
}
```

```typescript
// hooks/useAssetAudio.ts

import { useState, useEffect } from 'react';
import { useApi } from './useApi';

export function useAssetAudio(assetObjectId: string | null) {
  const api = useApi();
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    if (!assetObjectId) {
      setAudioUrl(null);
      return;
    }

    let cancelled = false;
    let objectUrl: string | null = null;

    async function loadAudio() {
      setLoading(true);
      setError(null);

      try {
        objectUrl = await api.getAssetUrl(assetObjectId);
        if (!cancelled) {
          setAudioUrl(objectUrl);
        }
      } catch (e) {
        if (!cancelled) {
          setError(e as Error);
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    }

    loadAudio();

    return () => {
      cancelled = true;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [api, assetObjectId]);

  return { audioUrl, loading, error };
}
```

### 5. Audio Player Component

```typescript
// components/AssetAudioPlayer.tsx

import { useAssetAudio } from '../hooks/useAssetAudio';

interface AssetAudioPlayerProps {
  assetObjectId: string;
  autoPlay?: boolean;
}

export function AssetAudioPlayer({ assetObjectId, autoPlay = false }: AssetAudioPlayerProps) {
  const { audioUrl, loading, error } = useAssetAudio(assetObjectId);

  if (loading) {
    return <div>Loading audio...</div>;
  }

  if (error) {
    return <div>Error loading audio: {error.message}</div>;
  }

  if (!audioUrl) {
    return null;
  }

  return (
    <audio
      src={audioUrl}
      controls
      autoPlay={autoPlay}
    />
  );
}
```

### 6. Dispatch Flow Example

```typescript
// Example: Triggering TTS via dispatch and playing result

import { useState } from 'react';
import { useApi } from '../hooks/useApi';
import { AssetAudioPlayer } from '../components/AssetAudioPlayer';

export function TTSDemo() {
  const api = useApi();
  const [assetObjectId, setAssetObjectId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleGenerateSpeech() {
    setLoading(true);
    try {
      // 1. Trigger dispatch (which runs workflow with TTS prompt)
      const dispatch = await api.triggerDispatch(
        'journey-object-id',
        'step-with-tts-object-id'
      );

      // 2. Poll for completion (or use websocket)
      let result;
      while (true) {
        result = await api.getDispatchResult(dispatch.object_id);
        if (result.status === 'ok' || result.status === 'failed') break;
        await new Promise(r => setTimeout(r, 1000));
      }

      // 3. Extract asset_object_id from dispatch result
      // (The workflow stores this in the result payload)
      const audioAssetId = result.payload?.audio?.asset_object_id;

      if (audioAssetId) {
        setAssetObjectId(audioAssetId);
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <div>
      <button onClick={handleGenerateSpeech} disabled={loading}>
        {loading ? 'Generating...' : 'Generate Speech'}
      </button>

      {assetObjectId && (
        <AssetAudioPlayer assetObjectId={assetObjectId} autoPlay />
      )}
    </div>
  );
}
```

## Migration Checklist

### Remove Direct Connections

- [ ] Remove `CONDUIT_URL` from env and code
- [ ] Remove `DATABASE_URL` from env and code
- [ ] Remove `REDIS_URL` from env and code
- [ ] Remove any TTS-specific env vars
- [ ] Remove any direct fetch calls to non-API URLs

### Search for violations

```bash
# Run these in dapple-ui directory
grep -rE "(localhost|127\.0\.0\.1):(8081|5432|6379)" --include="*.ts" --include="*.tsx"
grep -ri "conduit\|synthesize" --include="*.ts" --include="*.tsx"
grep -ri "database_url\|redis_url" --include="*.ts" --include="*.tsx" --include="*.env*"
```

### Update Existing Code

- [ ] Replace direct TTS calls with dispatch triggering
- [ ] Replace direct asset fetches with API client methods
- [ ] Add AuthProvider to app root
- [ ] Update all API calls to use ApiClient with JWT

## File Structure

```
dapple-ui/
├── lib/
│   ├── config.ts              # API URL only
│   └── api-client.ts          # ApiClient class
├── providers/
│   └── AuthProvider.tsx       # JWT auth context
├── hooks/
│   ├── useApi.ts              # API client hook
│   └── useAssetAudio.ts       # Audio loading hook
└── components/
    └── AssetAudioPlayer.tsx   # Audio player component
```

## Testing

1. **Auth flow**: Sign in with wallet, verify JWT is received
2. **Dispatch**: Trigger a workflow, verify it completes
3. **Asset content**: Fetch audio asset, verify playback works
4. **Error handling**: Test with invalid JWT, missing assets, etc.

## Notes

### Security Considerations

- JWT stored in localStorage (consider httpOnly cookie for production)
- All requests authenticated via Authorization header
- Asset access validated server-side via journey/dispatch ownership

### Performance Considerations

- Use `URL.createObjectURL()` for audio/images to avoid re-fetching
- Remember to `URL.revokeObjectURL()` when components unmount
- Consider caching asset metadata separately from content
