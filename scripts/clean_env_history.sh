#!/bin/bash
# Script to check and clean .env from Git history
# WARNING: This rewrites Git history and should be used with caution.
# Backup your repository before proceeding.

# Check if .env is in history
echo "Checking if .env exists in Git history..."
if git log --name-only --pretty=format:"" | grep -q "^.env$"; then
  echo ".env found in Git history. Cleaning now..."
  # Use git filter-branch to remove .env
  git filter-branch --force --index-filter \
    'git rm --cached --ignore-unmatch .env' \
    --prune-empty --tag-name-filter cat -- --all
  echo "History cleaned. Force push to remote with: git push origin --force --all"
else
  echo "No .env found in Git history. You're safe."
fi
