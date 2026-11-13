# Error messages for dapple-prompt documentation
# Language: Brazilian Portuguese (pt-BR)
# Status: Placeholder - Needs Translation

# HTTP Status Codes
error-400 = Requisição Inválida
error-400-desc = A requisição estava malformada ou continha dados inválidos.

error-401 = Não Autorizado
error-401-desc = Autenticação é necessária ou as credenciais fornecidas são inválidas.

error-403 = Proibido
error-403-desc = Você não tem permissão para acessar este recurso.

error-404 = Não Encontrado
error-404-desc = O recurso solicitado não existe.

error-409 = Conflito
error-409-desc = A requisição conflita com o estado atual (ex: chave duplicada).

error-500 = Erro Interno do Servidor
error-500-desc = Ocorreu um erro inesperado no servidor.

error-503 = Serviço Indisponível
error-503-desc = O serviço está temporariamente indisponível.

# Authentication Errors
error-auth-malformed-token = Token Malformado
error-auth-malformed-token-desc = A estrutura do token é inválida.

error-auth-invalid-signature = Assinatura Inválida
error-auth-invalid-signature-desc = A verificação da assinatura falhou.

error-auth-expired = Token Expirado
error-auth-expired-desc = O token passou do tempo de expiração.

error-auth-not-yet-valid = Token Ainda Não Válido
error-auth-not-yet-valid-desc = O horário atual é anterior ao timestamp valid_from do token.

error-auth-signer-not-authorized = Assinante Não Autorizado
error-auth-signer-not-authorized-desc = O endereço do assinante não está na lista de permitidos.

error-auth-service-not-found = Serviço Não Encontrado
error-auth-service-not-found-desc = Nenhum token de serviço ativo encontrado para o service_id fornecido.

error-auth-already-revoked = Já Revogado
error-auth-already-revoked-desc = O token já foi revogado.

# Validation Errors
error-validation-required = Campo Obrigatório Ausente
error-validation-required-desc = Um campo obrigatório não foi fornecido.

error-validation-format = Formato Inválido
error-validation-format-desc = O valor do campo não corresponde ao formato esperado.

error-validation-range = Valor Fora do Intervalo
error-validation-range-desc = O valor está fora do intervalo permitido.
