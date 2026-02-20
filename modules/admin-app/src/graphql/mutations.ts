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

export const CREATE_CITY = gql`
  mutation CreateCity($location: String!) {
    createCity(location: $location) {
      success
      slug
    }
  }
`;

export const ADD_SOURCE = gql`
  mutation AddSource($citySlug: String!, $url: String!, $reason: String) {
    addSource(citySlug: $citySlug, url: $url, reason: $reason) {
      success
      sourceId
    }
  }
`;

export const RUN_SCOUT = gql`
  mutation RunScout($citySlug: String!) {
    runScout(citySlug: $citySlug) {
      success
      message
    }
  }
`;

export const STOP_SCOUT = gql`
  mutation StopScout($citySlug: String!) {
    stopScout(citySlug: $citySlug) {
      success
      message
    }
  }
`;

export const RESET_SCOUT_LOCK = gql`
  mutation ResetScoutLock($citySlug: String!) {
    resetScoutLock(citySlug: $citySlug) {
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
  mutation SubmitSource($url: String!, $description: String, $city: String) {
    submitSource(url: $url, description: $description, city: $city) {
      success
      sourceId
    }
  }
`;
