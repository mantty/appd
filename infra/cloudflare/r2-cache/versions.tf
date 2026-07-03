terraform {
  required_version = ">= 1.8.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "6.42.0"
    }

    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "5.20.0"
    }
  }
}
