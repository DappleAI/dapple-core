PostgreSQL 18 Migration Architecture Plan                                                                                                                                           
                                                                                                                                                                                    
Status: Draft                                                                                                                                                                       
Created: 2026-02-23                                                                                                                                                                 
Last Updated: 2026-02-23                                                                                                                                                            
Target Version: PG18 + UUID-only PKs                                                                                                                                                
Dependencies: None (greenfield PG18 instance, no in-place upgrade)                                                                                                                  
                                                                                                                                                                                    
---                                                                                                                                                                                 
Executive Summary                                                                                                                                                                   
                                                                                                                                                                                    
This plan covers the migration of the dapple-prompt system from PostgreSQL 16 to PostgreSQL 18, accompanied by three structural changes: (1) renaming the K8s database service from 
dapple-postgres to prompt-db, (2) renaming environment variables from DAPPLE_DATABASE_URL to DAPPLE_PROMPT_DATABASE_URL, and (3) eliminating the dual-key pattern (id BIGINT +      
object_id UUID) in favor of a single object_id UUID DEFAULT uuidv7() PRIMARY KEY on every table.                                                                                    
                                                                                                                                                                                    
The dual-key elimination is the dominant work item. It touches every model struct (~80+), every SQL query file (~120+), every query function (~37), every handler (~35), all 7      
declarative macros, 5 core traits, and the MergeOrdered trait used for parent-child folding. The infrastructure and env-var changes are comparatively small but must happen first so
 the new PG18 instance is reachable.                                                                                                                                                
                                                                                                                                                                                    
Fresh consolidated migrations will be written for the PG18 instance. The 65 legacy migrations will not be carried forward. All seed/lookup data will be preserved in the new        
migration set.                                                                                                                                                                      
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue Breakdown                                                                                                                                                                     
                                                                                                                                                                                    
This plan is organized as 12 discrete issues. Issues 1-3 are infrastructure prerequisites that can be done in parallel. Issues 4-8 form the core data-layer rewrite and must be done
 sequentially. Issues 9-11 consume the data-layer changes. Issue 12 is the final deployment verification.                                                                           
                                                                                                                                                                                    
Dependency Graph                                                                                                                                                                    
                                                                                                                                                                                    
Issue 1 (K8s PG18 Infra)  ─┐                                                                                                                                                        
Issue 2 (Env Var Rename)   ─┼── Issue 3 (Consolidated Migrations) ──┐                                                                                                               
                            │                                        │                                                                                                              
                            │   Issue 4 (Traits + Macros) ───────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 5 (Model Structs) ─────────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 6 (SQL Query Files) ───────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 7 (Query Functions) ───────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 8 (API Handlers) ──────────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 9 (Integration Tests) ─────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 10 (Documentation) ────────────┤                                                                                                              
                            │       │                                │                                                                                                              
                            │   Issue 11 (Deployment + Verify) ──────┘                                                                                                              
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 1: PostgreSQL 18 Kubernetes Infrastructure                                                                                                                                    
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Deploy PostgreSQL 18 as prompt-db in Kubernetes                                                                                                                                     
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Create a new PG18 instance alongside the existing PG16 instance. The new instance uses the service name prompt-db (anticipating multiple DB instances in the future). The old       
dapple-postgres instance remains running until cutover is verified.                                                                                                                 
                                                                                                                                                                                    
Changes                                                                                                                                                                             
                                                                                                                                                                                    
New files:                                                                                                                                                                          
- assets/k8s/registry/prompt-db.yaml -- New manifest based on postgres.yaml                                                                                                         
- assets/k8s/registry/prompt-db-secret.yaml -- New secret                                                                                                                           
                                                                                                                                                                                    
assets/k8s/registry/prompt-db.yaml:                                                                                                                                                 
apiVersion: v1                                                                                                                                                                      
kind: Secret                                                                                                                                                                        
metadata:                                                                                                                                                                           
  name: prompt-db-secret                                                                                                                                                            
  namespace: dapple-sandbox                                                                                                                                                         
type: Opaque                                                                                                                                                                        
stringData:                                                                                                                                                                         
  POSTGRES_PASSWORD: "ai"                                                                                                                                                           
---                                                                                                                                                                                 
apiVersion: v1                                                                                                                                                                      
kind: PersistentVolumeClaim                                                                                                                                                         
metadata:                                                                                                                                                                           
  name: prompt-db-pvc                                                                                                                                                               
  namespace: dapple-sandbox                                                                                                                                                         
spec:                                                                                                                                                                               
  accessModes: [ReadWriteOnce]                                                                                                                                                      
  resources:                                                                                                                                                                        
    requests:                                                                                                                                                                       
      storage: 5Gi                                                                                                                                                                  
  storageClassName: microk8s-hostpath                                                                                                                                               
---                                                                                                                                                                                 
apiVersion: v1                                                                                                                                                                      
kind: ConfigMap                                                                                                                                                                     
metadata:                                                                                                                                                                           
  name: prompt-db-init-sql                                                                                                                                                          
  namespace: dapple-sandbox                                                                                                                                                         
data:                                                                                                                                                                               
  01-extensions.sql: |                                                                                                                                                              
    CREATE EXTENSION IF NOT EXISTS hstore;                                                                                                                                          
---                                                                                                                                                                                 
apiVersion: apps/v1                                                                                                                                                                 
kind: Deployment                                                                                                                                                                    
metadata:                                                                                                                                                                           
  name: prompt-db                                                                                                                                                                   
  namespace: dapple-sandbox                                                                                                                                                         
  labels: { app: prompt-db }                                                                                                                                                        
spec:                                                                                                                                                                               
  replicas: 1                                                                                                                                                                       
  selector:                                                                                                                                                                         
    matchLabels: { app: prompt-db }                                                                                                                                                 
  template:                                                                                                                                                                         
    metadata:                                                                                                                                                                       
      labels: { app: prompt-db }                                                                                                                                                    
    spec:                                                                                                                                                                           
      securityContext:                                                                                                                                                              
        fsGroup: 999                                                                                                                                                                
        fsGroupChangePolicy: OnRootMismatch                                                                                                                                         
      containers:                                                                                                                                                                   
        - name: prompt-db                                                                                                                                                           
          image: postgres:18.2-trixie                                                                                                        
          imagePullPolicy: IfNotPresent                                                                                                                                             
          ports:                                                                                                                                                                    
            - containerPort: 5432                                                                                                                                                   
          env:                                                                                                                                                                      
            - { name: POSTGRES_DB, value: "dapple" }                                                                                                                                
            - { name: POSTGRES_USER, value: "dapple" }                                                                                                                              
            - { name: POSTGRES_PASSWORD, valueFrom: { secretKeyRef: { name: prompt-db-secret, key: POSTGRES_PASSWORD } } }                                                          
          resources:                                                                                                                                                                
            requests: { cpu: "250m", memory: "256Mi" }                                                                                                                              
            limits:   { cpu: "1",    memory: "1Gi" }                                                                                                                                
          volumeMounts:                                                                                                                                                             
            - name: pg-data                                                                                                                                                         
              mountPath: /var/lib/postgresql/data                                                                                                                                   
            - name: init-sql                                                                                                                                                        
              mountPath: /docker-entrypoint-initdb.d                                                                                                                                
          readinessProbe:                                                                                                                                                           
            tcpSocket: { port: 5432 }                                                                                                                                               
            initialDelaySeconds: 5                                                                                                                                                  
            periodSeconds: 5                                                                                                                                                        
          livenessProbe:                                                                                                                                                            
            tcpSocket: { port: 5432 }                                                                                                                                               
            initialDelaySeconds: 20                                                                                                                                                 
            periodSeconds: 10                                                                                                                                                       
      volumes:                                                                                                                                                                      
        - name: pg-data                                                                                                                                                             
          persistentVolumeClaim:                                                                                                                                                    
            claimName: prompt-db-pvc                                                                                                                                                
        - name: init-sql                                                                                                                                                            
          configMap:                                                                                                                                                                
            name: prompt-db-init-sql                                                                                                                                                
---                                                                                                                                                                                 
apiVersion: v1                                                                                                                                                                      
kind: Service                                                                                                                                                                       
metadata:                                                                                                                                                                           
  name: prompt-db                                                                                                                                                                   
  namespace: dapple-sandbox                                                                                                                                                         
spec:                                                                                                                                                                               
  type: ClusterIP                                                                                                                                                                   
  selector: { app: prompt-db }                                                                                                                                                      
  ports:                                                                                                                                                                            
    - name: postgres                                                                                                                                                                
      port: 5432                                                                                                                                                                    
      targetPort: 5432                                                                                                                                                              
---                                                                                                                                                                                 
# Gateway DaemonSet for external access (same pattern as dapple-postgres-gateway)                                                                                                   
apiVersion: v1                                                                                                                                                                      
kind: ServiceAccount                                                                                                                                                                
metadata:                                                                                                                                                                           
  name: prompt-db-gateway                                                                                                                                                           
  namespace: dapple-sandbox                                                                                                                                                         
---                                                                                                                                                                                 
apiVersion: rbac.authorization.k8s.io/v1                                                                                                                                            
kind: Role                                                                                                                                                                          
metadata:                                                                                                                                                                           
  name: prompt-db-gateway                                                                                                                                                           
  namespace: dapple-sandbox                                                                                                                                                         
rules:                                                                                                                                                                              
  - apiGroups: [""]                                                                                                                                                                 
    resources: ["services"]                                                                                                                                                         
    verbs: ["get", "list"]                                                                                                                                                          
---                                                                                                                                                                                 
apiVersion: rbac.authorization.k8s.io/v1                                                                                                                                            
kind: RoleBinding                                                                                                                                                                   
metadata:                                                                                                                                                                           
  name: prompt-db-gateway                                                                                                                                                           
  namespace: dapple-sandbox                                                                                                                                                         
subjects:                                                                                                                                                                           
  - kind: ServiceAccount                                                                                                                                                            
    name: prompt-db-gateway                                                                                                                                                         
    namespace: dapple-sandbox                                                                                                                                                       
roleRef:                                                                                                                                                                            
  kind: Role                                                                                                                                                                        
  name: prompt-db-gateway                                                                                                                                                           
  apiGroup: rbac.authorization.k8s.io                                                                                                                                               
---                                                                                                                                                                                 
apiVersion: apps/v1                                                                                                                                                                 
kind: DaemonSet                                                                                                                                                                     
metadata:                                                                                                                                                                           
  name: prompt-db-gateway                                                                                                                                                           
  namespace: dapple-sandbox                                                                                                                                                         
spec:                                                                                                                                                                               
  selector:                                                                                                                                                                         
    matchLabels: { app: prompt-db-gateway }                                                                                                                                         
  template:                                                                                                                                                                         
    metadata:                                                                                                                                                                       
      labels: { app: prompt-db-gateway }                                                                                                                                            
    spec:                                                                                                                                                                           
      serviceAccountName: prompt-db-gateway                                                                                                                                         
      hostNetwork: true                                                                                                                                                             
      dnsPolicy: ClusterFirstWithHostNet                                                                                                                                            
      securityContext:                                                                                                                                                              
        runAsNonRoot: true                                                                                                                                                          
        runAsUser: 65534                                                                                                                                                            
        runAsGroup: 65534                                                                                                                                                           
      initContainers:                                                                                                                                                               
        - name: resolve-service-ip                                                                                                                                                  
          image: curlimages/curl:8.5.0                                                                                                                                              
          command:                                                                                                                                                                  
            - sh                                                                                                                                                                    
            - -c                                                                                                                                                                    
            - |                                                                                                                                                                     
              SERVICE_DNS="prompt-db.dapple-sandbox.svc.cluster.local"                                                                                                              
              echo "$SERVICE_DNS" > /shared/service-ip                                                                                                                              
          volumeMounts:                                                                                                                                                             
            - name: shared-data                                                                                                                                                     
              mountPath: /shared                                                                                                                                                    
      containers:                                                                                                                                                                   
        - name: prompt-db-gateway                                                                                                                                                   
          image: alpine/socat:1.8.0.0                                                                                                                                               
          command:                                                                                                                                                                  
            - sh                                                                                                                                                                    
            - -c                                                                                                                                                                    
            - |                                                                                                                                                                     
              SERVICE_IP=$(cat /shared/service-ip)                                                                                                                                  
              exec socat tcp-listen:5432,reuseaddr,fork tcp:$SERVICE_IP:5432                                                                                                        
          volumeMounts:                                                                                                                                                             
            - name: shared-data                                                                                                                                                     
              mountPath: /shared                                                                                                                                                    
          readinessProbe:                                                                                                                                                           
            tcpSocket: { port: 5432 }                                                                                                                                               
            initialDelaySeconds: 1                                                                                                                                                  
            periodSeconds: 3                                                                                                                                                        
          securityContext:                                                                                                                                                          
            allowPrivilegeEscalation: false                                                                                                                                         
            capabilities:                                                                                                                                                           
              add: ["NET_BIND_SERVICE"]                                                                                                                                             
              drop: ["ALL"]                                                                                                                                                         
          resources:                                                                                                                                                                
            requests: { cpu: "10m", memory: "16Mi" }                                                                                                                                
            limits:   { cpu: "100m", memory: "64Mi" }                                                                                                                               
      volumes:                                                                                                                                                                      
        - name: shared-data                                                                                                                                                         
          emptyDir: {}                                                                                                                                                              
                                                                                                                                                                                    
assets/k8s/registry/prompt-db-secret.yaml:                                                                                                                                          
apiVersion: v1                                                                                                                                                                      
kind: Secret                                                                                                                                                                        
metadata:                                                                                                                                                                           
  name: prompt-db                                                                                                                                                                   
  namespace: dapple-sandbox                                                                                                                                                         
type: Opaque                                                                                                                                                                        
stringData:                                                                                                                                                                         
  DAPPLE_PROMPT_DATABASE_URL: postgres://dapple:ai@prompt-db:5432/dapple                                                                                                            
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- assets/k8s/registry/prompt-db.yaml (new)                                                                                                                                          
- assets/k8s/registry/prompt-db-secret.yaml (new)                                                                                                                                   
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
None.                                                                                                                                                                               
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- prompt-db pod is running PG18 and healthy                                                                                                                                         
- prompt-db ClusterIP service is reachable from other pods                                                                                                                          
- prompt-db-gateway DaemonSet provides host-network access                                                                                                                          
- prompt-db secret contains correct connection string                                                                                                                               
- Old dapple-postgres remains running (parallel operation)                                                                                                                          
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 2: Environment Variable Rename                                                                                                                                                
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Rename DAPPLE_DATABASE_URL to DAPPLE_PROMPT_DATABASE_URL across all layers                                                                                                          
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Rename the database environment variable from DAPPLE_DATABASE_URL to DAPPLE_PROMPT_DATABASE_URL and update the connection string to point at prompt-db instead of dapple-postgres.  
Update the K8s secret name from dapple-db to prompt-db.                                                                                                                             
                                                                                                                                                                                    
Changes                                                                                                                                                                             
                                                                                                                                                                                    
Rust source code (2 files):                                                                                                                                                         
                                                                                                                                                                                    
┌────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────┐                                                            
│                    File                    │                                 Change                                  │                                                            
├────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤                                                            
│ crates/dapple-data/src/lib.rs              │ DAPPLE_DATABASE_URL -> DAPPLE_PROMPT_DATABASE_URL in DbPool::connect()  │                                                            
├────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────┤                                                            
│ crates/dapple-prompt-conduit/src/config.rs │ DAPPLE_DATABASE_URL -> DAPPLE_PROMPT_DATABASE_URL in Config::from_env() │                                                            
└────────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────┘                                                            
                                                                                                                                                                                    
K8s registry manifests (5 files) -- update secret references:                                                                                                                       
                                                                                                                                                                                    
┌────────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────┐                                    
│                      File                      │                                           Change                                            │                                    
├────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤                                    
│ assets/k8s/registry/dapple-prompt-api.yaml     │ secretKeyRef: dapple-db -> prompt-db, key DAPPLE_DATABASE_URL -> DAPPLE_PROMPT_DATABASE_URL │                                    
├────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤                                    
│ assets/k8s/registry/dapple-prompt-conduit.yaml │ Same                                                                                        │                                    
├────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤                                    
│ assets/k8s/registry/dapple-prompt-flow.yaml    │ Same                                                                                        │                                    
├────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤                                    
│ assets/k8s/registry/dapple-ui.yaml             │ Uses dapple-ui-auth secret -- see dapple-ui coordination note                               │                                    
└────────────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────┘                                    
                                                                                                                                                                                    
K8s manifest templates (2 files) -- update hardcoded URLs:                                                                                                                          
                                                                                                                                                                                    
┌─────────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────────────┐                           
│                      File                       │                                               Change                                                │                           
├─────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┤                           
│ assets/k8s/manifests/dapple-prompt-service.yaml │ Change DATABASE_URL env var to DAPPLE_PROMPT_DATABASE_URL, update host dapple-postgres -> prompt-db │                           
├─────────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────────────┤                           
│ assets/k8s/manifests/dapple-prompt-engine.yaml  │ Same                                                                                                │                           
└─────────────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────────────┘                           
                                                                                                                                                                                    
Nushell scripts and modules (6 files):                                                                                                                                              
                                                                                                                                                                                    
File: assets/nu/modules/dapple_env_profiles/dapple_sandbox.nu                                                                                                                       
Change: Rename DAPPLE_INTERNAL_POSTGRES_URL, DAPPLE_EXTERNAL_POSTGRES_URL, DAPPLE_DATABASE_URL to DAPPLE_PROMPT_* variants; update hostname dapple-postgres -> prompt-db            
────────────────────────────────────────                                                                                                                                            
File: assets/nu/init/setup_nu.nu                                                                                                                                                    
Change: Update PWD hook to set DAPPLE_PROMPT_DATABASE_URL instead of DATABASE_URL                                                                                                   
────────────────────────────────────────                                                                                                                                            
File: assets/nu/k8s/deploy_dapple_prompt_pods.nu                                                                                                                                    
Change: Update references to DATABASE_URL -> DAPPLE_PROMPT_DATABASE_URL; update DAPPLE_INTERNAL_POSTGRES_URL -> DAPPLE_PROMPT_INTERNAL_POSTGRES_URL                                 
────────────────────────────────────────                                                                                                                                            
File: assets/nu/k8s/build_image.nu                                                                                                                                                  
Change: Update DAPPLE_DATABASE_URL build arg                                                                                                                                        
────────────────────────────────────────                                                                                                                                            
File: assets/nu/k8s/deploy_dapple_web_pods.nu                                                                                                                                       
Change: Update DAPPLE_DATABASE_URL in dapple-ui auth secret creation                                                                                                                
────────────────────────────────────────                                                                                                                                            
File: assets/nu/modules/dapple/env.nu                                                                                                                                               
Change: Update dapple-ui config references                                                                                                                                          
                                                                                                                                                                                    
Note on dapple-ui: The dapple-ui Next.js app reads DAPPLE_DATABASE_URL from its environment. Keep backward-compatible alias in the K8s secret until dapple-ui is updated.           
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- crates/dapple-data/src/lib.rs                                                                                                                                                     
- crates/dapple-prompt-conduit/src/config.rs                                                                                                                                        
- assets/k8s/registry/dapple-prompt-api.yaml                                                                                                                                        
- assets/k8s/registry/dapple-prompt-conduit.yaml                                                                                                                                    
- assets/k8s/registry/dapple-prompt-flow.yaml                                                                                                                                       
- assets/k8s/manifests/dapple-prompt-service.yaml                                                                                                                                   
- assets/k8s/manifests/dapple-prompt-engine.yaml                                                                                                                                    
- assets/nu/modules/dapple_env_profiles/dapple_sandbox.nu                                                                                                                           
- assets/nu/init/setup_nu.nu                                                                                                                                                        
- assets/nu/k8s/deploy_dapple_prompt_pods.nu                                                                                                                                        
- assets/nu/k8s/build_image.nu                                                                                                                                                      
- assets/nu/k8s/deploy_dapple_web_pods.nu                                                                                                                                           
- assets/nu/modules/dapple/env.nu                                                                                                                                                   
- .env and .env.test                                                                                                                                                                
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 1 (prompt-db must exist for connection string to resolve)                                                                                                                   
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- All Rust crates read DAPPLE_PROMPT_DATABASE_URL                                                                                                                                   
- All K8s manifests reference prompt-db secret with correct key name                                                                                                                
- Nushell env profiles export DAPPLE_PROMPT_DATABASE_URL                                                                                                                            
- cargo build --workspace succeeds with new env var name                                                                                                                            
- Old dapple-postgres references are completely removed (grep verification)                                                                                                         
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 3: Consolidated Migrations for PG18                                                                                                                                           
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Write fresh consolidated migrations with UUID-only primary keys and all seed data                                                                                                   
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Write a new set of consolidated migrations for the PG18 instance. These replace all 65 legacy migrations. Every table uses object_id UUID DEFAULT uuidv7() PRIMARY KEY as its sole  
primary key (using PG18 native uuidv7()). Lookup tables keep id SMALLINT GENERATED ALWAYS AS IDENTITY.                                                                              
                                                                                                                                                                                    
Migration File Organization                                                                                                                                                         
                                                                                                                                                                                    
migrations/                                                                                                                                                                         
  00000000000001_schemas_and_extensions.sql    -- Schemas, extensions, slugify function                                                                                             
  00000000000002_lookup_tables.sql             -- Tier 0: All enum/lookup tables + seed data                                                                                        
  00000000000003_versioning_infrastructure.sql -- Tier 1-2: object_paths, versioned_objects, path_details                                                                           
  00000000000004_entity_tables.sql             -- Tier 2: All versioned entity tables                                                                                               
  00000000000005_user_layer.sql                -- Tier 3: user_details, roles, tokens                                                                                               
  00000000000006_playbook_structure.sql        -- Tier 4: playbook_stages, steps, tasks, directives                                                                                 
  00000000000007_journey_execution.sql         -- Tier 5: journeys, dispatches, attempts                                                                                            
  00000000000008_brief_system.sql              -- Tier 6: brief_units, groups, packs                                                                                                
  00000000000009_intent_bindings.sql           -- Tier 7: dispatch_intent_bindings + typed items                                                                                    
  00000000000010_asset_storage.sql             -- Tier 8: drafts, chunks, asset_chunks                                                                                              
  00000000000011_fluent_and_prompts.sql        -- Tier 9-10: fluent messages, prompts, inference profiles                                                                           
  00000000000012_guide_system.sql              -- Tier 11: guides, chapters, blocks, tags, cards, renders                                                                           
  00000000000013_canonical_pins.sql            -- Canonical release pins + ledger                                                                                                   
                                                                                                                                                                                    
Key DDL Changes                                                                                                                                                                     
                                                                                                                                                                                    
Before (PG16):                                                                                                                                                                      
CREATE TABLE rev.tasks (                                                                                                                                                            
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,                                                                                                                             
    object_id UUID NOT NULL DEFAULT uuidv7() UNIQUE,                                                                                                                                
    versioned_object_id BIGINT NOT NULL REFERENCES rev.versioned_objects(id),                                                                                                       
    ...                                                                                                                                                                             
);                                                                                                                                                                                  
                                                                                                                                                                                    
After (PG18):                                                                                                                                                                       
CREATE TABLE rev.tasks (                                                                                                                                                            
    object_id UUID DEFAULT uuidv7() PRIMARY KEY,                                                                                                                                    
    versioned_object_id UUID NOT NULL REFERENCES rev.versioned_objects(object_id),                                                                                                  
    ...                                                                                                                                                                             
);                                                                                                                                                                                  
                                                                                                                                                                                    
FK pattern change:                                                                                                                                                                  
-- Before                                                                                                                                                                           
task_id BIGINT NOT NULL REFERENCES rev.tasks(id)                                                                                                                                    
-- After                                                                                                                                                                            
task_id UUID NOT NULL REFERENCES rev.tasks(object_id)                                                                                                                               
                                                                                                                                                                                    
Lookup tables keep SMALLINT id (entity_types, deprecation_levels, value_kinds, asset_kinds, etc.) -- they are small, static, and referenced by integer FK for compactness.          
                                                                                                                                                                                    
Seed Data Preservation                                                                                                                                                              
                                                                                                                                                                                    
All seed data from existing migrations must be preserved:                                                                                                                           
- rev.entity_types -- task, step, stage, playbook, prompt, fluent_message, category, hat, brief, brief_group, brief_pack, value_slot, asset, directive, guide, inference_profile    
- rev.deprecation_levels -- active, deprecated, retired, quarantined                                                                                                                
- rev.task_attempt_phases -- queued, running, failed, ok, canceled                                                                                                                  
- rev.value_kinds -- text, int, number, bool, json, asset                                                                                                                           
- rev.asset_kinds -- image, document, audio, video, binary                                                                                                                          
- rev.brief_unit_types -- output, markdown, notice, input, media                                                                                                                    
- rev.brief_input_control_types -- text, textarea, select, radio, checkbox, number, date, file                                                                                      
- rev.brief_output_source_kinds -- input, files, computed                                                                                                                           
- web.role_types -- user, admin                                                                                                                                                     
- rev.providers -- anthropic, openai, mistral, self-hosted, stub                                                                                                                    
- rev.lineages -- vits, flan-t5, modernbert, claude-3-5, claude-4-6, mistral                                                                                                        
- rev.model_releases -- all provider+lineage+tier combinations                                                                                                                      
- rev.fluent_message_locales -- en-US, pt-PT                                                                                                                                        
- rev.guide_tags -- agent, product, coder, auditor, planner                                                                                                                         
- rev.guide_block_directive_types -- prose, stage, step, nushell, ref, block, brief_pack, inference_profile                                                                         
                                                                                                                                                                                    
What Is NOT Needed                                                                                                                                                                  
                                                                                                                                                                                    
- Custom uuidv7() SQL function (PG18 native)                                                                                                                                        
- pgcrypto extension (PG18 gen_random_uuid() is built-in)                                                                                                                           
- Legacy data migration scripts                                                                                                                                                     
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 1 (PG18 instance must exist)                                                                                                                                                
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- cargo sqlx migrate run succeeds against fresh PG18 database                                                                                                                       
- All 4 schemas exist (auth, rev, web, public)                                                                                                                                      
- All lookup tables are populated with correct seed data                                                                                                                            
- SELECT uuidv7() works natively (no custom function)                                                                                                                               
- No table has id BIGINT GENERATED ALWAYS AS IDENTITY (except lookup tables)                                                                                                        
- Every non-lookup table has object_id UUID DEFAULT uuidv7() PRIMARY KEY                                                                                                            
- All FK constraints reference UUID PKs                                                                                                                                             
- All FK columns have explicit indexes                                                                                                                                              
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 4: Trait System and Macro Overhaul                                                                                                                                            
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Rewrite core traits and macros to eliminate id: i64                                                                                                                                 
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
The foundation of the data layer rests on 5 core traits and 7 macros that all assume the dual-key pattern. This issue rewrites them to work with UUID-only primary keys.            
                                                                                                                                                                                    
Trait Changes                                                                                                                                                                       
                                                                                                                                                                                    
Keyed trait (lib.rs):                                                                                                                                                               
// Before:                                                                                                                                                                          
pub trait Keyed { fn id(&self) -> &i64; fn object_id(&self) -> &Uuid; }                                                                                                             
// After:                                                                                                                                                                           
pub trait Keyed { fn object_id(&self) -> &Uuid; }                                                                                                                                   
                                                                                                                                                                                    
RelationalObject trait (traits.rs):                                                                                                                                                 
// Before:                                                                                                                                                                          
pub trait RelationalObject { fn id(&self) -> &i64; fn object_id(&self) -> &Uuid; fn as_pair(&self) -> (i64, Uuid); }                                                                
// After:                                                                                                                                                                           
pub trait RelationalObject { fn object_id(&self) -> &Uuid; }                                                                                                                        
                                                                                                                                                                                    
OwnedDependent trait (traits.rs):                                                                                                                                                   
// Before:                                                                                                                                                                          
pub trait OwnedDependent { fn parent_id(&self) -> &i64; fn parent_object_id(&self) -> &Uuid; }                                                                                      
// After:                                                                                                                                                                           
pub trait OwnedDependent { fn parent_id(&self) -> &Uuid; }                                                                                                                          
                                                                                                                                                                                    
UnifiedVersionedObject trait (traits.rs):                                                                                                                                           
// Before: fn versioned_object_id(&self) -> i64;                                                                                                                                    
// After: fn versioned_object_id(&self) -> &Uuid;                                                                                                                                   
                                                                                                                                                                                    
MergeOrdered trait (lib.rs): Change parent_id/child_id from &i64 to &Uuid. UUIDv7 bytes sort chronologically so the merge algorithm works correctly.                                
                                                                                                                                                                                    
Macro Changes                                                                                                                                                                       
                                                                                                                                                                                    
┌──────────────────────────┬─────────────────────────────────────────────────────┐                                                                                                  
│          Macro           │                       Change                        │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_keyed!              │ Remove id() impl, keep only object_id()             │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_versioned!          │ versioned_object_id returns &Uuid                   │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_flat_to_parent!     │ Call sites remove id from copy lists                │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_insert_column_args! │ No structural change (types change at struct level) │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_principal_traits!   │ Cascades from keyed/versioned changes               │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ validate_insert_args!    │ No change                                           │                                                                                                  
├──────────────────────────┼─────────────────────────────────────────────────────┤                                                                                                  
│ impl_dto_object_id!      │ No change                                           │                                                                                                  
└──────────────────────────┴─────────────────────────────────────────────────────┘                                                                                                  
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- crates/dapple-data/src/lib.rs                                                                                                                                                     
- crates/dapple-data/src/traits.rs                                                                                                                                                  
- crates/dapple-data/src/macros.rs                                                                                                                                                  
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 3 (DDL must be finalized)                                                                                                                                                   
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- Keyed trait has no id() method                                                                                                                                                    
- MergeOrdered uses &Uuid for parent/child matching                                                                                                                                 
- All macros compile with new trait signatures                                                                                                                                      
- No references to i64 in trait definitions (except lookup table models)                                                                                                            
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 5: Model Struct Updates                                                                                                                                                       
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Update all ~80+ model structs to eliminate id: i64 and change FK fields to UUID                                                                                                     
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Every model struct in crates/dapple-data/src/model/ needs updating:                                                                                                                 
- Remove id: i64 fields                                                                                                                                                             
- Change FK fields from i64 to Uuid                                                                                                                                                 
- Change versioned_object_id from i64 to Uuid                                                                                                                                       
- Remove redundant path_id: i64 (merge with path_object_id: Uuid)                                                                                                                   
- Update all fold_models, MergeOrdered impls, impl_flat_to_parent! call sites                                                                                                       
                                                                                                                                                                                    
Change Pattern (applied to every entity)                                                                                                                                            
                                                                                                                                                                                    
// Before:                                                                                                                                                                          
pub struct TaskModel { pub id: i64, pub object_id: Uuid, pub versioned_object_id: i64, pub path_id: i64, pub path_object_id: Uuid, ... }                                            
// After:                                                                                                                                                                           
pub struct TaskModel { pub object_id: Uuid, pub versioned_object_id: Uuid, pub path_object_id: Uuid, ... }                                                                          
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- All files in crates/dapple-data/src/model/ (~40+ files, ~80+ structs)                                                                                                             
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 4 (traits and macros must be updated first)                                                                                                                                 
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No model struct has pub id: i64 (except lookup table models with id: i16)                                                                                                         
- All FK fields are Uuid                                                                                                                                                            
- All fold_models key by Uuid                                                                                                                                                       
- All MergeOrdered impls use &Uuid                                                                                                                                                  
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 6: SQL Query File Rewrite                                                                                                                                                     
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Rewrite all ~120+ SQL query files for UUID-only primary keys                                                                                                                        
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Every SQL query file needs rewriting:                                                                                                                                               
1. Remove id from SELECT/RETURNING clauses                                                                                                                                          
2. Change JOIN conditions from .id = to .object_id =                                                                                                                                
3. Change FK parameter types from $N::BIGINT[] to $N::UUID[]                                                                                                                        
4. Remove CTE steps that resolved object_id -> id                                                                                                                                   
5. Delete get_*_internal_ids.sql files (no longer needed)                                                                                                                           
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- All files under crates/dapple-data/assets/sql/ (~120+ files)                                                                                                                      
- Delete: brief_groups/get_brief_group_internal_ids.sql, briefs/get_brief_internal_ids.sql, fluent_messages/get_fluent_message_internal_ids.sql                                     
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 3 (DDL must be finalized)                                                                                                                                                   
- Issue 5 (model struct field names determine RETURNING clauses)                                                                                                                    
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No SQL file references id BIGINT columns (except lookup tables)                                                                                                                   
- All FK JOINs use .object_id =                                                                                                                                                     
- All UNNEST parameters use $N::UUID[] for FK columns                                                                                                                               
- cargo sqlx prepare --workspace succeeds against PG18                                                                                                                              
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 7: Query Function Updates                                                                                                                                                     
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Update all Rust query functions for UUID-only data flow                                                                                                                             
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
All query functions in crates/dapple-data/src/query/:                                                                                                                               
- InsertArgs FK fields change from Vec<i64> to Vec<Uuid>                                                                                                                            
- Remove ID-resolution helper functions                                                                                                                                             
- Update folding functions to key by Uuid                                                                                                                                           
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- All files in crates/dapple-data/src/query/ (~37 files)                                                                                                                            
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 5 (model structs), Issue 6 (SQL files)                                                                                                                                      
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No query function accepts or returns i64 for entity PKs/FKs                                                                                                                       
- All ID-resolution helpers removed                                                                                                                                                 
- cargo sqlx prepare --workspace succeeds                                                                                                                                           
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 8: API Handler Simplification                                                                                                                                                 
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Simplify API handlers by removing object_id-to-internal-id resolution                                                                                                               
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
The big win: POST handlers no longer need to look up shared dependents to get internal i64 IDs. The UUIDs from the API request ARE the foreign keys.                                
                                                                                                                                                                                    
// Before: resolve object_id -> internal id, then build InsertArgs with i64                                                                                                         
// After: UUIDs from API request used directly as FK values                                                                                                                         
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- All files in crates/dapple-prompt-api/src/handler/ (~35 files)                                                                                                                    
- Verify crates/dapple-prompt-api/src/schema/ for any leaked i64 fields                                                                                                             
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 7 (query functions must be updated)                                                                                                                                         
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No handler performs object_id -> internal id resolution                                                                                                                           
- No handler builds HashMap<Uuid, i64> lookup maps                                                                                                                                  
- cargo build -p dapple-prompt-api succeeds                                                                                                                                         
- Same API request/response shapes preserved                                                                                                                                        
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 9: Integration Test Updates                                                                                                                                                   
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Update all integration tests for UUID-only data model                                                                                                                               
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Update consumer tests and lifecycle trials. Since DTOs already use object_id: Uuid, most changes are in test helpers that build InsertArgs.                                         
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- crates/dapple-prompt-api/tests/consumer/*.rs                                                                                                                                      
- crates/dapple-prompt-api/tests/user_lifecycle.rs                                                                                                                                  
- crates/dapple-prompt-flow/tests/dispatch_flow_integration.rs                                                                                                                      
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 8                                                                                                                                                                           
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- cargo test --test user-lifecycle --package dapple-prompt-api passes                                                                                                               
- cargo test -p dapple-prompt-flow passes                                                                                                                                           
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 10: Documentation Updates                                                                                                                                                     
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Update AGENTS.md, CLAUDE.md, and README files for new conventions                                                                                                                   
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Update all documentation to reflect UUID-only PK convention, new env var names, and new K8s service name.                                                                           
                                                                                                                                                                                    
Files Affected                                                                                                                                                                      
                                                                                                                                                                                    
- CLAUDE.md, AGENTS.md                                                                                                                                                              
- crates/dapple-data/AGENTS.md                                                                                                                                                      
- crates/dapple-prompt-api/AGENTS.md                                                                                                                                                
- assets/nu/AGENTS.md, assets/k8s/AGENTS.md                                                                                                                                         
- .github/instructions/*.instructions.md                                                                                                                                            
- .github/copilot-instructions.md                                                                                                                                                   
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issues 4-9                                                                                                                                                                        
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No documentation references id: i64 for non-lookup tables                                                                                                                         
- No documentation references DAPPLE_DATABASE_URL or dapple-postgres                                                                                                                
- All code examples use UUID-only patterns                                                                                                                                          
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 11: Deployment and Verification                                                                                                                                               
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Deploy PG18 migration, run consolidated migrations, verify end-to-end                                                                                                               
                                                                                                                                                                                    
Description                                                                                                                                                                         
                                                                                                                                                                                    
Deploy PG18, run migrations, build, test, deploy services, smoke test, decommission PG16.                                                                                           
                                                                                                                                                                                    
Deployment Sequence                                                                                                                                                                 
                                                                                                                                                                                    
1. Deploy PG18 instance (Issue 1 artifacts)                                                                                                                                         
2. Wait for pod ready                                                                                                                                                               
3. Run consolidated migrations                                                                                                                                                      
4. Prepare SQLx cache                                                                                                                                                               
5. Build all crates                                                                                                                                                                 
6. Run full test suite                                                                                                                                                              
7. Deploy services                                                                                                                                                                  
8. Verify API health                                                                                                                                                                
9. Run smoke tests (create task -> playbook -> journey -> dispatch)                                                                                                                 
10. Decommission old PG16                                                                                                                                                           
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- All previous issues (1-10)                                                                                                                                                        
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- Full system runs on PG18 with UUID-only PKs                                                                                                                                       
- All tests pass                                                                                                                                                                    
- API health check returns 200                                                                                                                                                      
- Full lifecycle (task -> playbook -> journey -> dispatch) works                                                                                                                    
                                                                                                                                                                                    
---                                                                                                                                                                                 
Issue 12: Archive Legacy Migrations and Clean Up                                                                                                                                    
                                                                                                                                                                                    
Title                                                                                                                                                                               
                                                                                                                                                                                    
Archive legacy migration files and clean up dead code                                                                                                                               
                                                                                                                                                                                    
Changes                                                                                                                                                                             
                                                                                                                                                                                    
1. Move legacy migrations to crates/dapple-data/migrations_legacy/                                                                                                                  
2. Delete old K8s manifests (postgres.yaml, dapple-db-secret.yaml)                                                                                                                  
3. Remove dead code flagged by cargo clippy                                                                                                                                         
4. Regenerate .sqlx/ cache                                                                                                                                                          
                                                                                                                                                                                    
Dependencies                                                                                                                                                                        
                                                                                                                                                                                    
- Issue 11                                                                                                                                                                          
                                                                                                                                                                                    
Acceptance Criteria                                                                                                                                                                 
                                                                                                                                                                                    
- No legacy migration files in active directory                                                                                                                                     
- No dapple-postgres K8s manifests                                                                                                                                                  
- cargo clippy -- -D warnings passes                                                                                                                                                
                                                                                                                                                                                    
---                                                                                                                                                                                 
Risk Assessment                                                                                                                                                                     
                                                                                                                                                                                    
High Risk                                                                                                                                                                           
                                                                                                                                                                                    
- MergeOrdered UUID comparison semantics -- UUIDv7 bytes sort chronologically, so merge algorithm works. But concurrent uuidv7() calls could break ordering. Mitigation: PG18's     
uuidv7() is monotonic within a transaction.                                                                                                                                         
- sqlx type inference during transition -- Many files have mismatched types until both SQL and Rust sides are updated. Mitigation: Do Issues 5+6 together, entity by entity.        
                                                                                                                                                                                    
Medium Risk                                                                                                                                                                         
                                                                                                                                                                                    
- PG18 stability -- May still be beta/RC. Pin to stable once released.                                                                                                              
- dapple-ui coordination -- Reads DAPPLE_DATABASE_URL. Keep backward-compatible alias until updated.                                                                                
                                                                                                                                                                                    
Low Risk                                                                                                                                                                            
                                                                                                                                                                                    
- Lookup table PKs -- Keeping SMALLINT id is a conscious exception, already handled differently in code.                                                                            
                                                                                                                                                                                    
---                                                                                                                                                                                 
Suggested Implementation Order                                                                                                                                                      
                                                                                                                                                                                    
For Issues 4-9, use entity-by-entity vertical slices: for each entity, update model -> SQL -> query function -> handler -> test. Entity dependency order:                           
                                                                                                                                                                                    
1. Versioning infrastructure (object_paths, versioned_objects)                                                                                                                      
2. Root entities (tasks, steps, stages, categories, hats)                                                                                                                           
3. Composite entities (playbooks with stages/steps/tasks)                                                                                                                           
4. User/auth (journeys, dispatches, tokens)                                                                                                                                         
5. Brief system                                                                                                                                                                     
6. Guides                                                                                                                                                                           
                                                                                                                                                                                    
---                                                                                                                                                                                 
Open Questions                                                                                                                                                                      
                                                                                                                                                                                    
1. ~~PG18 image tag~~ — Resolved: using `postgres:18.2-trixie` (stable, Debian Trixie base).                                                                                   
2. dapple-ui env var rename timeline -- Parallel update or backward-compatible alias?                                                                                               
3. Migration file naming -- Placeholder timestamps vs real timestamps for sqlx migrate?                                                                                             
4. path_id vs path_object_id naming -- FK column path_id UUID in DDL, aliased as path_object_id in SQL SELECT?                                                                      
5. auth schema -- Include CREATE SCHEMA auth but let NextAuth manage its own tables?                                                                                                
