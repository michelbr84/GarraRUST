#!/bin/bash
# Script para atualizar issues no Linear
# Uso: LINEAR_API_KEY=your_api_key ./update_linear.sh

API_KEY=${LINEAR_API_KEY:-""}

if [ -z "$API_KEY" ]; then
    echo "Erro: Defina a variavel LINEAR_API_KEY"
    echo "Obtenha em: https://linear.app/settings/api"
    echo "Exemplo: LINEAR_API_KEY=lin_api_xxx ./update_linear.sh"
    exit 1
fi

echo "Atualizando issues no Linear..."

# Lista de issues para marcar como done
ISSUES=("GAR-170" "GAR-165" "GAR-166" "GAR-167" "GAR-168" "GAR-169" 
        "GAR-160" "GAR-162" "GAR-163" "GAR-158" "GAR-164" "GAR-161"
        "GAR-171" "GAR-172" "GAR-157" "GAR-173" "GAR-174" "GAR-175" "GAR-176")

for issue in "${ISSUES[@]}"; do
    echo "Marcando $issue como done..."
    curl -s -X POST "https://api.linear.app/graphql" \
        -H "Authorization: $API_KEY" \
        -H "Content-Type: application/json" \
        -d "{\"query\": \"mutation { updateIssue(input: {id: \\\"$issue\\\", stateId: \\\"done\\\"}) { success } }\"}" | jq -r '.data.updateIssue.success'
done

echo ""
echo "Concluido! Verifique o Linear para confirmar."
