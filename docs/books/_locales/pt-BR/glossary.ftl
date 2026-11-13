# Glossary terms for dapple-prompt documentation
# Language: Brazilian Portuguese (pt-BR)
# Status: Placeholder - Needs Translation

# Core Execution Model
term-task = Tarefa
term-task-def = Uma única unidade de trabalho implementada como um
    pequeno script. Tarefas têm entradas, saídas e comportamento
    de saída claros.

term-step = Etapa de Configuração
term-step-def = Uma subinteração dinâmica dentro de um Estágio (um ciclo
    de trabalho, mais qualquer entrada do operador) antes de o usuário
    avançar. A forma canônica é gerar, escolher, refinar: uma tarefa gera
    candidatos, o operador escolhe um e uma etapa posterior refina a
    escolha. Estágios com várias etapas são a norma.

term-stage = Estágio
term-stage-def = Um formulário ou painel estático onde o usuário chega;
    avançar para o próximo estágio é a verdadeira navegação. Um Estágio
    contém uma ou mais Etapas de Configuração, as subinterações que
    acontecem nesse painel.

term-playbook = Playbook
term-playbook-def = Uma orquestração voltada ao usuário que combina
    Estágios em um fluxo completo.

term-playbook-instance = Instância de Playbook / Execução
term-playbook-instance-def = Uma única execução de um Playbook.
    O runtime cria uma instância por execução iniciada pelo usuário.

term-journey = Jornada
term-journey-def = Uma história ou experiência em nível de produto
    composta por um ou mais Playbooks, decisões de UX, regras de
    ramificação e estado de longa duração.

# Briefs & Packs
term-brief = Brief
term-brief-def = Um contêiner versionado para elementos de UI (entradas,
    saídas, markdown) usado em entrevistas de debrief.

term-brief-group = Grupo de Brief
term-brief-group-def = Um agrupamento lógico de Briefs relacionados que
    devem ser completados atomicamente.

term-brief-pack = Pacote de Brief
term-brief-pack-def = Um pacote de Playbooks, Briefs, prompts,
    entradas de exemplo e metadados destinados à distribuição.

term-pack-manifest = Manifesto do Pacote
term-pack-manifest-def = O arquivo de manifesto (YAML/JSON) na raiz
    de um Pacote de Brief descrevendo playbooks, prompts, exemplos,
    binários necessários e metadados.

# Artifacts & Files
term-artifact = Artefato
term-artifact-def = Qualquer arquivo ou saída estruturada produzida
    por uma Tarefa (ex: um currículo Markdown, um PDF, um arquivo
    de metadados JSON).

term-artifact-json = artifact.json
term-artifact-json-def = O arquivo de contrato mínimo que uma Tarefa
    deve escrever ao produzir um artefato principal.

# Prompts & LLM
term-prompt = Prompt
term-prompt-def = Um modelo de texto (frequentemente com placeholders)
    enviado a um LLM para extração, normalização ou geração de conteúdo.

term-prompt-template = Template de Prompt
term-prompt-template-def = Um arquivo de prompt reutilizável que contém
    placeholders como {{user_text}} ou {{input_json}} destinados
    à substituição em tempo de execução.

term-conduit = Conduit
term-conduit-def = A ponte de runtime que roteia templates de prompt
    e prompts substituídos para um provedor LLM configurado.

term-overlay = Overlay
term-overlay-def = Um pacote de configuração de runtime que sobrescreve
    o roteamento de prompt/conduit sem editar o prompt.

# Authentication
term-pat = Token de Acesso Pessoal (PAT)
term-pat-def = Um token com escopo de usuário, armazenado por hash,
    para acesso programático à API. Prefixado com dpl_ para identificação.

term-jwt = JWT (JSON Web Token)
term-jwt-def = Um token autocontido que codifica claims e pode ser
    validado sem consulta ao banco de dados.

term-service-token = Token de Serviço
term-service-token-def = Um token assinado com Ed25519 para autenticação
    serviço-a-serviço entre serviços internos.
