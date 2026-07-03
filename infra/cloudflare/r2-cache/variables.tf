variable "cloudflare_api_token" {
  description = "Cloudflare API token with Workers R2 Storage Write permission."
  sensitive   = true
  type        = string
}

variable "cloudflare_account_id" {
  description = "Cloudflare account ID that owns the R2 bucket."
  type        = string
}

variable "r2_access_key_id" {
  description = "R2 S3 access key ID used by the AWS provider to manage lifecycle rules."
  sensitive   = true
  type        = string
}

variable "r2_secret_access_key" {
  description = "R2 S3 secret access key used by the AWS provider to manage lifecycle rules."
  sensitive   = true
  type        = string
}

variable "bucket_name" {
  default     = "appd-workerd-bazel-cache"
  description = "R2 bucket name for the appd workerd Bazel cache."
  type        = string
}

variable "bucket_location" {
  default     = null
  description = "Optional R2 location hint. Leave null for Cloudflare automatic placement."
  type        = string

  validation {
    condition = (
      var.bucket_location == null ||
      contains(["apac", "eeur", "enam", "weur", "wnam", "oc"], var.bucket_location)
    )
    error_message = "bucket_location must be one of apac, eeur, enam, weur, wnam, oc, or null."
  }
}

variable "bucket_jurisdiction" {
  default     = "default"
  description = "R2 jurisdiction guarantee."
  type        = string

  validation {
    condition     = contains(["default", "eu", "fedramp"], var.bucket_jurisdiction)
    error_message = "bucket_jurisdiction must be default, eu, or fedramp."
  }
}

variable "storage_class" {
  default     = "Standard"
  description = "Default storage class for new R2 objects."
  type        = string

  validation {
    condition     = contains(["Standard", "InfrequentAccess"], var.storage_class)
    error_message = "storage_class must be Standard or InfrequentAccess."
  }
}

variable "cache_prefix" {
  default     = "bazel/appd-workerd"
  description = "Object prefix used by bazel-remote inside the bucket."
  type        = string
}

variable "lifecycle_expiration_days" {
  default     = 30
  description = "Days before cached Bazel objects expire from R2."
  type        = number

  validation {
    condition     = var.lifecycle_expiration_days >= 1
    error_message = "lifecycle_expiration_days must be at least 1."
  }
}

variable "abort_multipart_upload_days" {
  default     = 7
  description = "Days before incomplete multipart uploads are aborted."
  type        = number

  validation {
    condition     = var.abort_multipart_upload_days >= 1
    error_message = "abort_multipart_upload_days must be at least 1."
  }
}
