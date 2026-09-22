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

## Gateway credential (required since v0.4.5)

The image runs `garraia start --host 0.0.0.0`, and since v0.4.5 (#1261)
garraia **refuses** a non-loopback bind without a gateway credential: the
task exits 78 and never passes its health check. Store a key in Secrets
Manager (or an SSM SecureString) and pass its ARN:

```bash
aws secretsmanager create-secret --name garraia/gateway-key \
  --secret-string "$(openssl rand -hex 32)"
```

```hcl
gateway_api_key_secret_arn = "arn:aws:secretsmanager:us-east-1:123456789:secret:garraia/gateway-key"
```

The module injects it as `GARRAIA_GATEWAY_API_KEY` and grants the execution
role read access to it (and to every ARN in `secrets`). Clients send it as
`Authorization: Bearer <key>` on `/api/*` and `/ws`; `/health` stays open for
the ALB and ECS probes. Upgrading an existing stack: create the secret and
set this variable **before** moving `container_tag` to v0.4.5 or later
(`latest` included).

## Usage

```bash
# Initialize
terraform init

# Plan
terraform plan -var="container_tag=v0.4.5" -var="gateway_api_key_secret_arn=arn:aws:secretsmanager:..."

# Apply
terraform apply -var="container_tag=v0.4.5" -var="gateway_api_key_secret_arn=arn:aws:secretsmanager:..."

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
| `gateway_api_key_secret_arn` | — (required) | ARN holding `GARRAIA_GATEWAY_API_KEY` (#1261) |
| `task_cpu` | `512` | Fargate CPU (512 = 0.5 vCPU) |
| `task_memory` | `1024` | Fargate memory (MiB) |
| `desired_count` | `1` | Number of tasks |
| `enable_autoscaling` | `false` | Enable HPA |
| `max_capacity` | `5` | Max tasks for scaling |
