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

variable "secrets" {
  description = "List of secret environment variables from SSM/Secrets Manager"
  type = list(object({
    name      = string
    valueFrom = string
  }))
  default = []
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
