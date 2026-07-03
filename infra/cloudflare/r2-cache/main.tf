provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

provider "aws" {
  access_key = var.r2_access_key_id
  secret_key = var.r2_secret_access_key
  region     = "us-east-1"

  s3_use_path_style = true

  skip_credentials_validation = true
  skip_region_validation      = true
  skip_requesting_account_id  = true

  endpoints {
    s3 = "https://${var.cloudflare_account_id}.r2.cloudflarestorage.com"
  }
}

resource "cloudflare_r2_bucket" "bazel_cache" {
  account_id    = var.cloudflare_account_id
  jurisdiction  = var.bucket_jurisdiction
  location      = var.bucket_location
  name          = var.bucket_name
  storage_class = var.storage_class
}

resource "aws_s3_bucket_lifecycle_configuration" "bazel_cache" {
  bucket = cloudflare_r2_bucket.bazel_cache.name

  rule {
    id     = "expire-bazel-cache-objects"
    status = "Enabled"

    filter {
      prefix = var.cache_prefix
    }

    expiration {
      days = var.lifecycle_expiration_days
    }
  }

  rule {
    id     = "abort-incomplete-bazel-cache-uploads"
    status = "Enabled"

    filter {
      prefix = var.cache_prefix
    }

    abort_incomplete_multipart_upload {
      days_after_initiation = var.abort_multipart_upload_days
    }
  }
}
