# GarraIA — Terraform (AWS ECS Fargate)

Deploy GarraIA to AWS using ECS Fargate with an Application Load Balancer.

## Architecture

- **VPC** with 2 public + 2 private subnets across AZs
- **ALB** in public subnets (HTTP:80)
- **ECS Fargate** tasks in private subnets
- **CloudWatch** for container logs
- Optional **auto scaling** based on CPU utilization

## Prerequisites

- Terraform >= 1.5
- AWS CLI configured (`aws configure`)
- Docker image pushed to GHCR or ECR

## Usage

```bash
# Initialize
terraform init

# Plan
terraform plan -var="container_tag=v0.3.0"

# Apply
terraform apply -var="container_tag=v0.3.0"

# Destroy
terraform destroy
```

## Passing Secrets

Use AWS Secrets Manager or SSM Parameter Store:

```hcl
secrets = [
  {
    name      = "GARRAIA_VAULT_PASSPHRASE"
    valueFrom = "arn:aws:secretsmanager:us-east-1:123456789:secret:garraia/vault-passphrase"
  },
  {
    name      = "GARRAIA_JWT_SECRET"
    valueFrom = "arn:aws:ssm:us-east-1:123456789:parameter/garraia/jwt-secret"
  }
]
```

## Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `project_name` | `garraia` | Resource naming prefix |
| `environment` | `prod` | dev / staging / prod |
| `aws_region` | `us-east-1` | AWS region |
| `container_image` | `ghcr.io/michelbr84/garraia` | Docker image |
| `container_tag` | `latest` | Image tag |
| `task_cpu` | `512` | Fargate CPU (512 = 0.5 vCPU) |
| `task_memory` | `1024` | Fargate memory (MiB) |
| `desired_count` | `1` | Number of tasks |
| `enable_autoscaling` | `false` | Enable HPA |
| `max_capacity` | `5` | Max tasks for scaling |
