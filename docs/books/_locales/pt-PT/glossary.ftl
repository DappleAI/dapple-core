# Glossary terms for dapple-prompt documentation
# Language: European Portuguese (pt-PT)
# Status: Placeholder - Needs Translation

# Core Execution Model
term-task = Tarefa
term-task-def = Uma única unidade de trabalho implementada como um
    pequeno script. Tarefas têm entradas, saídas e comportamento
    de saída claros.

term-step = Passo de Configuração
term-step-def = Uma subinteração dinâmica dentro de uma Etapa (um ciclo
    de trabalho, mais qualquer entrada do operador) antes de o utilizador
    avançar. A forma canónica é gerar, escolher, refinar: uma tarefa gera
    candidatos, o operador escolhe um e um passo posterior refina a
    escolha. Etapas com vários passos são a norma.

term-stage = Etapa
term-stage-def = Um formulário ou painel estático onde o utilizador
    chega; avançar para a etapa seguinte é a verdadeira navegação. Uma
    Etapa contém um ou mais Passos de Configuração, as subinterações que
    acontecem nesse painel.

term-playbook = Playbook
term-playbook-def = Uma orquestração voltada ao utilizador que combina
    Etapas num fluxo completo.

term-playbook-instance = Instância de Playbook / Execução
term-playbook-instance-def = Uma única execução de um Playbook.
    O runtime cria uma instância por execução iniciada pelo utilizador.

term-journey = Jornada
term-journey-def = Uma história ou experiência ao nível de produto
    composta por um ou mais Playbooks, decisões de UX, regras de
    ramificação e estado de longa duração.

# Briefs & Packs
term-brief = Brief
term-brief-def = Um contentor versionado para elementos de UI (entradas,
    saídas, markdown) utilizado em entrevistas de debrief.

term-brief-group = Grupo de Brief
term-brief-group-def = Um agrupamento lógico de Briefs relacionados que
    devem ser completados atomicamente.

term-brief-pack = Pacote de Brief
term-brief-pack-def = Um pacote de Playbooks, Briefs, prompts,
    entradas de exemplo e metadados destinados à distribuição.

term-pack-manifest = Manifesto do Pacote
term-pack-manifest-def = O ficheiro de manifesto (YAML/JSON) na raiz
    de um Pacote de Brief descrevendo playbooks, prompts, exemplos,
    binários necessários e metadados.

# Artifacts & Files
term-artifact = Artefacto
term-artifact-def = Qualquer ficheiro ou saída estruturada produzida
    por uma Tarefa (ex: um currículo Markdown, um PDF, um ficheiro
    de metadados JSON).

term-artifact-json = artifact.json
term-artifact-json-def = O ficheiro de contrato mínimo que uma Tarefa
    deve escrever ao produzir um artefacto principal.

# Prompts & LLM
term-prompt = Prompt
term-prompt-def = Um modelo de texto (frequentemente com placeholders)
    enviado a um LLM para extração, normalização ou geração de conteúdo.

term-prompt-template = Template de Prompt
term-prompt-template-def = Um ficheiro de prompt reutilizável que contém
    placeholders como {{user_text}} ou {{input_json}} destinados
    à substituição em tempo de execução.

term-conduit = Conduit
term-conduit-def = A ponte de runtime que encaminha templates de prompt
    e prompts substituídos para um fornecedor LLM configurado.

term-overlay = Overlay
term-overlay-def = Um pacote de configuração de runtime que sobrepõe
    o encaminhamento de prompt/conduit sem editar o prompt.

# Authentication
term-pat = Token de Acesso Pessoal (PAT)
term-pat-def = Um token com âmbito de utilizador, armazenado por hash,
    para acesso programático à API. Prefixado com dpl_ para identificação.

term-jwt = JWT (JSON Web Token)
term-jwt-def = Um token autocontido que codifica claims e pode ser
    validado sem consulta à base de dados.

term-service-token = Token de Serviço
term-service-token-def = Um token assinado com Ed25519 para autenticação
    serviço-a-serviço entre serviços internos.
