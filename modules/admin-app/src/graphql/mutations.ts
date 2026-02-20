import { gql } from "@apollo/client";

export const SEND_OTP = gql`
  mutation SendOtp($phone: String!) {
    sendOtp(phone: $phone) {
      success
    }
  }
`;

export const VERIFY_OTP = gql`
  mutation VerifyOtp($phone: String!, $code: String!) {
    verifyOtp(phone: $phone, code: $code) {
      success
    }
  }
`;

export const LOGOUT = gql`
  mutation Logout {
    logout {
      success
    }
  }
`;

export const CREATE_REGION = gql`
  mutation CreateCity($location: String!) {
    createCity(location: $location) {
      success
      slug
    }
  }
`;

export const ADD_SOURCE = gql`
  mutation AddSource($regionSlug: String!, $url: String!, $reason: String) {
    addSource(regionSlug: $regionSlug, url: $url, reason: $reason) {
      success
      sourceId
    }
  }
`;

export const RUN_SCOUT = gql`
  mutation RunScout($regionSlug: String!) {
    runScout(regionSlug: $regionSlug) {
      success
      message
    }
  }
`;

export const STOP_SCOUT = gql`
  mutation StopScout($regionSlug: String!) {
    stopScout(regionSlug: $regionSlug) {
      success
      message
    }
  }
`;

export const RESET_SCOUT_LOCK = gql`
  mutation ResetScoutLock($regionSlug: String!) {
    resetScoutLock(regionSlug: $regionSlug) {
      success
      message
    }
  }
`;

export const TAG_STORY = gql`
  mutation TagStory($storyId: UUID!, $tagSlug: String!) {
    tagStory(storyId: $storyId, tagSlug: $tagSlug) {
      success
    }
  }
`;

export const UNTAG_STORY = gql`
  mutation UntagStory($storyId: UUID!, $tagSlug: String!) {
    untagStory(storyId: $storyId, tagSlug: $tagSlug) {
      success
    }
  }
`;

export const MERGE_TAGS = gql`
  mutation MergeTags($sourceSlug: String!, $targetSlug: String!) {
    mergeTags(sourceSlug: $sourceSlug, targetSlug: $targetSlug) {
      success
    }
  }
`;

export const SUBMIT_SOURCE = gql`
  mutation SubmitSource($url: String!, $description: String, $region: String) {
    submitSource(url: $url, description: $description, region: $region) {
      success
      sourceId
    }
  }
`;
