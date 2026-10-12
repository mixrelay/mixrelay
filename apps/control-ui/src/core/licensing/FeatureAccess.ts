import { createContext, useContext } from 'react';

// Preview/story fixtures have no native entitlement. The desktop always supplies its status.
export const FeatureAccess = createContext(true);
export function usePaidFeatures() {
  return useContext(FeatureAccess);
}
