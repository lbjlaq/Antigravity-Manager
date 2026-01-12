import React, { useEffect, useState } from 'react';
import { getTopSignatures, SignatureStats } from '../../services/cacheService';

export const TopSignaturesTable: React.FC = () => {
  const [signatures, setSignatures] = useState<SignatureStats[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchSignatures = async () => {
      try {
        const data = await getTopSignatures(10);
        setSignatures(data);
      } catch (err) {
        console.error('Failed to fetch top signatures:', err);
      } finally {
        setLoading(false);
      }
    };

    fetchSignatures();
    const interval = setInterval(fetchSignatures, 10000); // Refresh every 10s

    return () => clearInterval(interval);
  }, []);

  if (loading) {
    return <div className="loading loading-spinner"></div>;
  }

  if (signatures.length === 0) {
    return (
      <div className="alert alert-info">
        <span>No cached signatures yet. Signatures will appear as requests are cached.</span>
      </div>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="table table-zebra w-full">
        <thead>
          <tr>
            <th>Signature Hash</th>
            <th>Reuse Count</th>
            <th>Cost Saved</th>
            <th>Avg Lookup (ms)</th>
            <th>Last Used</th>
          </tr>
        </thead>
        <tbody>
          {signatures.map((sig) => (
            <tr key={sig.signature}>
              <td>
                <code className="text-xs">{sig.signature.substring(0, 16)}...</code>
              </td>
              <td>
                <span className="badge badge-primary">{sig.reuse_count}</span>
              </td>
              <td>${sig.cost_saved.toFixed(4)}</td>
              <td>{sig.avg_lookup_time.toFixed(2)}</td>
              <td>
                <span className="text-xs text-gray-500">
                  {new Date(sig.last_used).toLocaleString()}
                </span>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
};
