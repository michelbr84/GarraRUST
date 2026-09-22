{{/*
Expand the name of the chart.
*/}}
{{- define "garraia.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "garraia.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "garraia.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels.
*/}}
{{- define "garraia.labels" -}}
helm.sh/chart: {{ include "garraia.chart" . }}
{{ include "garraia.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels.
*/}}
{{- define "garraia.selectorLabels" -}}
app.kubernetes.io/name: {{ include "garraia.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Service account name.
*/}}
{{- define "garraia.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "garraia.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Image reference.
*/}}
{{- define "garraia.image" -}}
{{- $tag := default .Chart.AppVersion .Values.image.tag -}}
{{- printf "%s:%s" .Values.image.repository $tag }}
{{- end }}

{{/*
Name of the Secret that holds GARRAIA_GATEWAY_API_KEY (#1261).
*/}}
{{- define "garraia.gatewayKeySecretName" -}}
{{- if .Values.gatewayApiKey.existingSecret }}
{{- .Values.gatewayApiKey.existingSecret }}
{{- else }}
{{- printf "%s-gateway-key" (include "garraia.fullname" .) | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}

{{/*
Key inside that Secret.
*/}}
{{- define "garraia.gatewayKeySecretKey" -}}
{{- if .Values.gatewayApiKey.existingSecret }}
{{- required "gatewayApiKey.existingSecretKey must name the key inside gatewayApiKey.existingSecret" .Values.gatewayApiKey.existingSecretKey }}
{{- else }}
{{- "GARRAIA_GATEWAY_API_KEY" }}
{{- end }}
{{- end }}
