# ============================================================================
# GarraIA — Terraform Variables
# ============================================================================

variable "project_name" {
  description = "Project name used for resource naming"
  type        = string
  default     = "garraia"
}

variable "environment" {
  description = "Deployment environment (dev, staging, prod)"
  type        = string
  default     = "prod"

  validation {
    condition     = contains(["dev", "staging", "prod"], var.environment)
    error_message = "Environment must be one of: dev, staging, prod."
  }
}

variable "aws_region" {
  description = "AWS region for deployment"
  type        = string
  default     = "us-east-1"
}

variable "vpc_cidr" {
  description = "CIDR block for VPC"
  type        = string
  default     = "10.0.0.0/16"
}

# ---------------------------------------------------------------------------
# Container
# ---------------------------------------------------------------------------

variable "container_image" {
  description = "Docker image repository"
  type        = string
  default     = "ghcr.io/michelbr84/garraia"
}

variable "container_tag" {
  description = "Docker image tag"
  type        = string
  default     = "latest"
}

variable "container_port" {
  description = "Port the container listens on"
  type        = number
  default     = 3888
}

# ---------------------------------------------------------------------------
# ECS Task
# ---------------------------------------------------------------------------

variable "task_cpu" {
  description = "Fargate task CPU units (256 = 0.25 vCPU)"
  type        = number
  default     = 512
}

variable "task_memory" {
  description = "Fargate task memory in MiB"
  type        = number
  default     = 1024
}

variable "desired_count" {
  description = "Number of ECS tasks to run"
  type        = number
  default     = 1
}

variable "log_level" {
  description = "Application log level"
  type        = string
  default     = "info"
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 30
}

# ---------------------------------------------------------------------------
# Secrets (from AWS Secrets Manager / SSM)
# ---------------------------------------------------------------------------

variable "gateway_api_key_secret_arn" {
  description = <<-EOT
    ARN of the Secrets Manager secret or SSM SecureString parameter holding
    GARRAIA_GATEWAY_API_KEY. REQUIRED since v0.4.5 (#1261): the image runs
    `garraia start --host 0.0.0.0`, and garraia refuses a non-loopback bind
    without a gateway credential (the task exits 78 and never turns healthy).
    Generate the value with `openssl rand -hex 32`.
  EOT
  type        = string

  validation {
    condition     = can(regex("^arn:aws[a-z-]*:(secretsmanager|ssm):", var.gateway_api_key_secret_arn))
    error_message = "gateway_api_key_secret_arn must be a Secrets Manager or SSM parameter ARN (#1261)."
  }
}

variable "secrets" {
  description = <<-EOT
    Extra secret environment variables from SSM/Secrets Manager. The gateway
    credential goes in gateway_api_key_secret_arn, not here.
  EOT
  type = list(object({
    name      = string
    valueFrom = string
  }))
  default = []

  validation {
    condition     = !contains([for s in var.secrets : s.name], "GARRAIA_GATEWAY_API_KEY")
    error_message = "Pass GARRAIA_GATEWAY_API_KEY through gateway_api_key_secret_arn, not secrets (#1261)."
  }
}

# ---------------------------------------------------------------------------
# Auto Scaling
# ---------------------------------------------------------------------------

variable "enable_autoscaling" {
  description = "Enable ECS service auto scaling"
  type        = bool
  default     = false
}

variable "max_capacity" {
  description = "Maximum number of ECS tasks for auto scaling"
  type        = number
  default     = 5
}
