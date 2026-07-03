output "bucket_name" {
  description = "R2 bucket used for the appd workerd Bazel cache."
  value       = cloudflare_r2_bucket.bazel_cache.name
}

output "cache_prefix" {
  description = "Prefix used by bazel-remote inside the R2 bucket."
  value       = var.cache_prefix
}

output "r2_endpoint" {
  description = "S3-compatible R2 endpoint for bazel-remote."
  value       = "https://${var.cloudflare_account_id}.r2.cloudflarestorage.com"
}
