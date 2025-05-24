#!/bin/bash

# Navigate to the zinit project directory
cd /Users/despiegk/code/github/threefoldtech/zinit

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo "Error: Not in zinit project directory"
    exit 1
fi

# Function to get the latest tag from Git
get_latest_tag() {
    # Fetch all tags from remote
    git fetch --tags origin 2>/dev/null
    
    # Get the latest tag using version sorting
    local latest_tag=$(git tag -l "v*" | sort -V | tail -n 1)
    
    if [ -z "$latest_tag" ]; then
        echo "v0.0.0"
    else
        echo "$latest_tag"
    fi
}

# Function to increment version
increment_version() {
    local version=$1
    # Remove 'v' prefix if present
    version=${version#v}
    
    # Split version into parts
    IFS='.' read -ra PARTS <<< "$version"
    major=${PARTS[0]:-0}
    minor=${PARTS[1]:-0}
    patch=${PARTS[2]:-0}
    
    # Increment patch (maintenance) version
    patch=$((patch + 1))
    
    echo "v${major}.${minor}.${patch}"
}

echo "🔍 Checking latest tag..."
latest_tag=$(get_latest_tag)
echo "Latest tag: $latest_tag"

new_version=$(increment_version "$latest_tag")
echo "New version: $new_version"

# Confirm with user
read -p "Create release $new_version? (y/N): " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Release cancelled"
    exit 0
fi

# Check if tag already exists locally and remove it
if git tag -l | grep -q "^$new_version$"; then
    echo "⚠️  Local tag $new_version already exists, removing it..."
    git tag -d "$new_version"
fi

# Make sure we're on the right branch and up to date
echo "🔄 Updating repository..."
git fetch origin

# Get current branch name
current_branch=$(git branch --show-current)

# If we're not on main or master, try to checkout one of them
if [[ "$current_branch" != "main" && "$current_branch" != "master" ]]; then
    echo "Current branch: $current_branch"
    if git show-ref --verify --quiet refs/heads/main; then
        echo "Switching to main branch..."
        git checkout main
        current_branch="main"
    elif git show-ref --verify --quiet refs/heads/master; then
        echo "Switching to master branch..."
        git checkout master
        current_branch="master"
    else
        echo "⚠️  Neither main nor master branch found, staying on current branch: $current_branch"
    fi
fi

echo "Pulling latest changes from $current_branch..."
git pull origin "$current_branch"

# Create and push the tag
echo "🏷️  Creating tag $new_version..."
git tag "$new_version"

echo "🚀 Pushing tag to trigger release..."
git push origin "$new_version"

echo "✅ Release $new_version has been triggered!"
echo "🔗 Check the release at: https://github.com/threefoldtech/zinit/releases"
echo "🔗 Monitor the build at: https://github.com/threefoldtech/zinit/actions"
